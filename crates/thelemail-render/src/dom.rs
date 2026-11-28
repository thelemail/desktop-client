use std::borrow::Cow;
use std::cell::{Cell, Ref, RefCell};
use std::fmt;
use std::sync::LazyLock;

use html5ever::interface::{ElemName, ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{
    Attribute, LocalName, Namespace, ParseOpts, QualName, local_name, ns, parse_document,
};

pub(crate) type NodeId = usize;

pub(crate) const DOCUMENT: NodeId = 0;

#[derive(Clone, Debug)]
pub(crate) struct Attr {
    pub name: String,
    pub value: String,
}

#[derive(Debug)]
pub(crate) enum Data {
    Document,
    Doctype,
    Fragment,
    Element {
        name: QualName,
        attrs: Vec<Attr>,
        template: Option<NodeId>,
        annotation_xml_html: bool,
    },
    Text(String),
    Comment(String),
}

#[derive(Debug)]
pub(crate) struct Node {
    pub data: Data,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev: Option<NodeId>,
    pub next: Option<NodeId>,
}

#[derive(Debug, Default)]
pub(crate) struct Dom {
    pub nodes: Vec<Node>,
    pub truncated: bool,
}

const MAX_PARSER_DEPTH: u32 = 4096;

const PARSER_WORK_BUDGET: u64 = 32_000_000;

const FEED_CHUNK: usize = 4096;

const VOID: [&str; 18] = [
    "area", "base", "basefont", "bgsound", "br", "col", "embed", "frame", "hr", "img", "input",
    "keygen", "link", "meta", "param", "source", "track", "wbr",
];

const RAW_TEXT_PARENTS: [&str; 7] = [
    "iframe",
    "noembed",
    "noframes",
    "plaintext",
    "script",
    "style",
    "xmp",
];

const NEWLINE_SENSITIVE: [&str; 3] = ["listing", "pre", "textarea"];

impl Dom {
    pub fn new_text(&mut self, text: String) -> NodeId {
        self.push(Data::Text(text))
    }

    fn push(&mut self, data: Data) -> NodeId {
        self.nodes.push(Node {
            data,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
        });
        self.nodes.len() - 1
    }

    pub fn element_name(&self, id: NodeId) -> Option<&QualName> {
        match &self.nodes[id].data {
            Data::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn is_html(&self, id: NodeId, local: &str) -> bool {
        self.element_name(id)
            .is_some_and(|name| name.ns == ns!(html) && &*name.local == local)
    }

    pub fn attrs(&self, id: NodeId) -> &[Attr] {
        match &self.nodes[id].data {
            Data::Element { attrs, .. } => attrs,
            _ => &[],
        }
    }

    pub fn attrs_mut(&mut self, id: NodeId) -> Option<&mut Vec<Attr>> {
        match &mut self.nodes[id].data {
            Data::Element { attrs, .. } => Some(attrs),
            _ => None,
        }
    }

    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.attrs(id)
            .iter()
            .find(|attr| attr.name == name)
            .map(|attr| attr.value.as_str())
    }

    pub fn set_attr(&mut self, id: NodeId, name: &str, value: &str) {
        if let Some(attrs) = self.attrs_mut(id) {
            match attrs.iter_mut().find(|attr| attr.name == name) {
                Some(attr) => attr.value = value.to_owned(),
                None => attrs.push(Attr {
                    name: name.to_owned(),
                    value: value.to_owned(),
                }),
            }
        }
    }

    pub fn template_contents(&self, id: NodeId) -> Option<NodeId> {
        match &self.nodes[id].data {
            Data::Element { template, .. } => *template,
            _ => None,
        }
    }

    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            dom: self,
            next: self.nodes[id].first_child,
        }
    }

    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].first_child
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id].next
    }

    pub fn detach(&mut self, id: NodeId) {
        let Some(parent) = self.nodes[id].parent.take() else {
            return;
        };
        let prev = self.nodes[id].prev.take();
        let next = self.nodes[id].next.take();
        match prev {
            Some(prev) => self.nodes[prev].next = next,
            None => self.nodes[parent].first_child = next,
        }
        match next {
            Some(next) => self.nodes[next].prev = prev,
            None => self.nodes[parent].last_child = prev,
        }
    }

    pub fn insert_before(&mut self, parent: NodeId, before: Option<NodeId>, child: NodeId) {
        self.detach(child);
        let prev = match before {
            Some(before) => self.nodes[before].prev,
            None => self.nodes[parent].last_child,
        };
        self.nodes[child].parent = Some(parent);
        self.nodes[child].prev = prev;
        self.nodes[child].next = before;
        match prev {
            Some(prev) => self.nodes[prev].next = Some(child),
            None => self.nodes[parent].first_child = Some(child),
        }
        match before {
            Some(before) => self.nodes[before].prev = Some(child),
            None => self.nodes[parent].last_child = Some(child),
        }
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        self.insert_before(parent, None, child);
    }

    pub fn hoist_children(&mut self, id: NodeId) {
        let Some(parent) = self.nodes[id].parent else {
            return;
        };
        let after = self.nodes[id].next;
        while let Some(child) = self.nodes[id].first_child {
            self.insert_before(parent, after, child);
        }
    }

    pub fn following(&self, id: NodeId, root: NodeId) -> Option<NodeId> {
        let mut current = id;
        loop {
            if current == root {
                return None;
            }
            if let Some(next) = self.next_sibling(current) {
                return Some(next);
            }
            current = self.nodes[current].parent?;
        }
    }

    pub fn next_in_tree(&self, id: NodeId, root: NodeId) -> Option<NodeId> {
        self.first_child(id).or_else(|| self.following(id, root))
    }

    pub fn first_element(&self, root: NodeId, local: &str) -> Option<NodeId> {
        let mut cursor = Some(root);
        while let Some(id) = cursor {
            if self.is_html(id, local) {
                return Some(id);
            }
            cursor = self.next_in_tree(id, root);
        }
        None
    }

    pub fn descendants(&self, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cursor = self.first_child(root);
        while let Some(id) = cursor {
            out.push(id);
            cursor = self.next_in_tree(id, root);
        }
        out
    }

    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for node in self.descendants(id) {
            if let Data::Text(text) = &self.nodes[node].data {
                out.push_str(text);
            }
        }
        out
    }

    pub fn inner_html(&self, id: NodeId) -> String {
        let mut out = String::new();
        self.serialize_children(id, false, &mut out);
        out
    }

    pub fn serialize_children(&self, root: NodeId, sorted_attrs: bool, out: &mut String) {
        let content_root = |id: NodeId| self.template_contents(id).unwrap_or(id);
        let mut stack: Vec<Step> = self.child_steps(content_root(root));
        while let Some(step) = stack.pop() {
            match step {
                Step::Close(id) => {
                    if let Some(name) = self.element_name(id) {
                        out.push_str("</");
                        push_tag_name(out, name);
                        out.push('>');
                    }
                }
                Step::Open(id) => match &self.nodes[id].data {
                    Data::Element { name, attrs, .. } => {
                        out.push('<');
                        push_tag_name(out, name);
                        let mut ordered: Vec<&Attr> = attrs.iter().collect();
                        if sorted_attrs {
                            ordered.sort_by(|a, b| a.name.cmp(&b.name).then(a.value.cmp(&b.value)));
                        }
                        for attr in ordered {
                            out.push(' ');
                            out.push_str(&attr.name);
                            out.push_str("=\"");
                            escape(out, &attr.value, true);
                            out.push('"');
                        }
                        out.push('>');
                        let html = name.ns == ns!(html);
                        if html && VOID.contains(&&*name.local) {
                            continue;
                        }
                        let content = content_root(id);
                        if html && NEWLINE_SENSITIVE.contains(&&*name.local) {
                            let first = self.first_child(content);
                            if let Some(Data::Text(text)) = first.map(|c| &self.nodes[c].data)
                                && text.starts_with('\n')
                            {
                                out.push('\n');
                            }
                        }
                        stack.push(Step::Close(id));
                        stack.extend(self.child_steps(content));
                    }
                    Data::Text(text) => {
                        let raw = self.nodes[id]
                            .parent
                            .and_then(|p| self.element_name(p))
                            .is_some_and(|name| {
                                name.ns == ns!(html) && RAW_TEXT_PARENTS.contains(&&*name.local)
                            });
                        if raw {
                            out.push_str(text);
                        } else {
                            escape(out, text, false);
                        }
                    }
                    Data::Comment(text) => {
                        out.push_str("<!--");
                        out.push_str(text);
                        out.push_str("-->");
                    }
                    Data::Document | Data::Doctype | Data::Fragment => {}
                },
            }
        }
    }
}

enum Step {
    Open(NodeId),
    Close(NodeId),
}

impl Dom {
    fn child_steps(&self, id: NodeId) -> Vec<Step> {
        let mut steps: Vec<Step> = self.children(id).map(Step::Open).collect();
        steps.reverse();
        steps
    }
}

pub(crate) struct Children<'a> {
    dom: &'a Dom,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.next?;
        self.next = self.dom.nodes[id].next;
        Some(id)
    }
}

fn push_tag_name(out: &mut String, name: &QualName) {
    if name.ns != ns!(html)
        && let Some(prefix) = &name.prefix
    {
        out.push_str(prefix);
        out.push(':');
    }
    out.push_str(&name.local);
}

fn escape(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '\u{A0}' => out.push_str("&nbsp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
}

pub(crate) fn attr_name(name: &QualName) -> String {
    match &name.prefix {
        Some(prefix) => format!("{prefix}:{}", name.local),
        None => name.local.to_string(),
    }
}

static NO_NAME: LazyLock<QualName> = LazyLock::new(|| QualName::new(None, ns!(), local_name!("")));

pub(crate) enum NameRef<'a> {
    Borrowed(Ref<'a, QualName>),
    Missing,
}

impl fmt::Debug for NameRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.qual())
    }
}

impl NameRef<'_> {
    fn qual(&self) -> &QualName {
        match self {
            NameRef::Borrowed(name) => name,
            NameRef::Missing => &NO_NAME,
        }
    }
}

impl ElemName for NameRef<'_> {
    fn ns(&self) -> &Namespace {
        &self.qual().ns
    }

    fn local_name(&self) -> &LocalName {
        &self.qual().local
    }
}

struct Sink {
    dom: RefCell<Dom>,
    depth: RefCell<Vec<u32>>,
    work: Cell<u64>,
    exceeded: std::rc::Rc<Cell<bool>>,
}

impl Sink {
    fn track(&self, parent: NodeId, child: NodeId) {
        let mut depth = self.depth.borrow_mut();
        let parent_depth = depth.get(parent).copied().unwrap_or(0);
        if depth.len() <= child {
            depth.resize(child + 1, 0);
        }
        depth[child] = parent_depth + 1;
        let work = self.work.get() + u64::from(parent_depth + 1);
        self.work.set(work);
        if parent_depth + 1 > MAX_PARSER_DEPTH || work > PARSER_WORK_BUDGET {
            self.exceeded.set(true);
        }
    }

    fn append_text(&self, parent: NodeId, before: Option<NodeId>, text: &str) {
        let mut dom = self.dom.borrow_mut();
        let previous = match before {
            Some(before) => dom.nodes[before].prev,
            None => dom.nodes[parent].last_child,
        };
        if let Some(previous) = previous
            && let Data::Text(existing) = &mut dom.nodes[previous].data
        {
            existing.push_str(text);
            return;
        }
        let id = dom.push(Data::Text(text.to_owned()));
        dom.insert_before(parent, before, id);
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Dom;
    type ElemName<'a> = NameRef<'a>;

    fn finish(self) -> Dom {
        self.dom.into_inner()
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> NameRef<'a> {
        let dom = self.dom.borrow();
        match Ref::filter_map(dom, |dom| dom.element_name(*target)) {
            Ok(name) => NameRef::Borrowed(name),
            Err(_) => NameRef::Missing,
        }
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> NodeId {
        let mut dom = self.dom.borrow_mut();
        let template = flags.template.then(|| dom.push(Data::Fragment));
        let attrs = attrs
            .into_iter()
            .map(|attr| Attr {
                name: attr_name(&attr.name),
                value: attr.value.to_string(),
            })
            .collect();
        dom.push(Data::Element {
            name,
            attrs,
            template,
            annotation_xml_html: flags.mathml_annotation_xml_integration_point,
        })
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.dom.borrow_mut().push(Data::Comment(text.to_string()))
    }

    fn create_pi(&self, _target: StrTendril, data: StrTendril) -> NodeId {
        self.dom.borrow_mut().push(Data::Comment(data.to_string()))
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        match child {
            NodeOrText::AppendNode(node) => {
                self.dom.borrow_mut().append_child(*parent, node);
                self.track(*parent, node);
            }
            NodeOrText::AppendText(text) => self.append_text(*parent, None, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        if self.dom.borrow().nodes[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
        let mut dom = self.dom.borrow_mut();
        let id = dom.push(Data::Doctype);
        dom.append_child(DOCUMENT, id);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.dom
            .borrow()
            .template_contents(*target)
            .unwrap_or(*target)
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let Some(parent) = self.dom.borrow().nodes[*sibling].parent else {
            return;
        };
        match new_node {
            NodeOrText::AppendNode(node) => {
                self.dom
                    .borrow_mut()
                    .insert_before(parent, Some(*sibling), node);
                self.track(parent, node);
            }
            NodeOrText::AppendText(text) => self.append_text(parent, Some(*sibling), &text),
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<Attribute>) {
        let mut dom = self.dom.borrow_mut();
        if let Some(existing) = dom.attrs_mut(*target) {
            for attr in attrs {
                let name = attr_name(&attr.name);
                if !existing.iter().any(|a| a.name == name) {
                    existing.push(Attr {
                        name,
                        value: attr.value.to_string(),
                    });
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.dom.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut dom = self.dom.borrow_mut();
        while let Some(child) = dom.nodes[*node].first_child {
            dom.append_child(*new_parent, child);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        matches!(
            self.dom.borrow().nodes[*handle].data,
            Data::Element {
                annotation_xml_html: true,
                ..
            }
        )
    }

    fn allow_declarative_shadow_roots(&self, _intended_parent: &NodeId) -> bool {
        false
    }
}

pub(crate) fn parse(html: &str) -> Dom {
    let mut dom = Dom::default();
    dom.push(Data::Document);
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let exceeded = std::rc::Rc::new(Cell::new(false));
    let sink = Sink {
        dom: RefCell::new(dom),
        depth: RefCell::new(Vec::new()),
        work: Cell::new(0),
        exceeded: exceeded.clone(),
    };
    let mut parser = parse_document(sink, opts);
    let mut rest = html;
    while !rest.is_empty() && !exceeded.get() {
        let mut cut = rest.len().min(FEED_CHUNK);
        while !rest.is_char_boundary(cut) {
            cut += 1;
        }
        parser.process(StrTendril::from_slice(&rest[..cut]));
        rest = &rest[cut..];
    }
    let mut dom = parser.finish();
    dom.truncated = exceeded.get();
    dom
}

#[cfg(test)]
pub(crate) fn canonical_body(html: &str) -> String {
    let mut dom = parse(&format!("<remove></remove>{html}"));
    let Some(body) = dom.first_element(DOCUMENT, "body") else {
        return String::new();
    };
    if let Some(first) = dom.first_child(body) {
        dom.detach(first);
    }
    for id in dom.descendants(body) {
        let newline_sensitive = dom
            .element_name(id)
            .is_some_and(|n| NEWLINE_SENSITIVE.contains(&&*n.local));
        if newline_sensitive
            && let Some(first) = dom.first_child(id)
            && let Data::Text(text) = &mut dom.nodes[first].data
            && let Some(rest) = text.strip_prefix('\n')
        {
            *text = rest.to_owned();
        }
    }
    let mut out = String::new();
    dom.serialize_children(body, true, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_html(html: &str) -> String {
        let dom = parse(&format!("<body>{html}"));
        let body = dom.first_element(DOCUMENT, "body").unwrap();
        dom.inner_html(body)
    }

    #[test]
    fn round_trips_ordinary_markup() {
        assert_eq!(
            body_html(
                "<p class=a title='x\"y'>a &amp; b<br>c</p><table><tr><td>1</td></tr></table>"
            ),
            "<p class=\"a\" title=\"x&quot;y\">a &amp; b<br>c</p><table><tbody><tr><td>1</td></tr></tbody></table>"
        );
    }

    #[test]
    fn style_text_is_raw_and_attributes_escape_angles() {
        assert_eq!(
            body_html("<style>a>b{}</style><p title='<x>'>"),
            "<style>a>b{}</style><p title=\"&lt;x&gt;\"></p>"
        );
    }

    #[test]
    fn pre_keeps_a_leading_newline_through_a_round_trip() {
        let once = body_html("<pre>\n\nx</pre>");
        assert_eq!(once, "<pre>\n\nx</pre>");
        assert_eq!(body_html(&once), once);
    }

    #[test]
    fn misnested_formatting_is_adopted() {
        assert_eq!(body_html("<b>1<p>2</b>3</p>"), "<b>1</b><p><b>2</b>3</p>");
    }

    #[test]
    fn template_contents_serialize() {
        assert_eq!(
            body_html("<template><p>x</p></template>"),
            "<template><p>x</p></template>"
        );
    }

    #[test]
    fn hostile_nesting_stops_the_parse() {
        let started = std::time::Instant::now();
        let dom = parse(&"<div>".repeat(200_000));
        assert!(dom.truncated);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let body = dom.first_element(DOCUMENT, "body").unwrap();
        assert!(dom.inner_html(body).starts_with("<div><div>"));
    }

    #[test]
    fn wide_documents_stay_whole() {
        let dom = parse(&"<p>x</p>".repeat(50_000));
        assert!(!dom.truncated);
    }
}
