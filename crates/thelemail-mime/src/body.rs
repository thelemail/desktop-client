use std::collections::BTreeMap;
use std::rc::Rc;

use encoding_rs::{Encoding, REPLACEMENT, UTF_8, WINDOWS_1252};
use mail_parser::{Address, MessageParser};

use crate::postal_text::{
    self, HeaderError, Mailbox, SubDate, SubMessage, format_html_header, format_text_header,
    html_to_text, js_trim, text_to_html,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlinePart {
    pub mime_type: String,
    pub data: Vec<u8>,
}

pub type InlineParts = BTreeMap<String, InlinePart>;

pub const MAX_MIME_DEPTH: usize = 256;

pub const MAX_EMBEDDED_MESSAGES: usize = 64;

const MAX_HEADERS_SIZE: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyError {
    TooDeep,
    HeadersTooLarge,
    EmbeddedHeader,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BodyOptions {
    pub utc_offset_seconds: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedBody {
    pub html: Option<String>,
    pub text: Option<String>,
    pub inline: InlineParts,
}

#[derive(Debug, Default, Clone)]
struct Structured {
    value: String,
    params: BTreeMap<String, String>,
}

enum Decoder {
    Pass(Vec<u8>),
    Base64(String),
    QuotedPrintable {
        charset: Option<String>,
        text: String,
    },
}

#[derive(PartialEq)]
enum State {
    Header,
    Body,
    Finished,
}

struct MimeNode {
    parent: Option<usize>,
    children: Vec<usize>,
    depth: usize,
    state: State,
    default_type: &'static str,
    header_lines: Vec<String>,
    header_size: usize,
    content_type: Structured,
    multipart: Option<String>,
    disposition: Structured,
    content_id: Option<String>,
    decoder: Decoder,
    content: Option<Vec<u8>>,
}

impl MimeNode {
    fn new(parent: Option<usize>, depth: usize, default_type: &'static str) -> Self {
        Self {
            parent,
            children: Vec::new(),
            depth,
            state: State::Header,
            default_type,
            header_lines: Vec::new(),
            header_size: 0,
            content_type: Structured::default(),
            multipart: None,
            disposition: Structured::default(),
            content_id: None,
            decoder: Decoder::Pass(Vec::new()),
            content: None,
        }
    }

    fn charset(&self) -> Option<&str> {
        self.content_type.params.get("charset").map(String::as_str)
    }

    fn text_content(&self) -> String {
        let Some(content) = &self.content else {
            return String::new();
        };
        let text = decode_charset(self.charset(), content);
        let flowed = self
            .content_type
            .params
            .get("format")
            .is_some_and(|f| f.eq_ignore_ascii_case("flowed"));
        if !flowed {
            return text;
        }
        let del_sp = self
            .content_type
            .params
            .get("delsp")
            .is_some_and(|d| d.eq_ignore_ascii_case("yes"));
        decode_flowed(&text, del_sp)
    }
}

fn encoding_for(charset: Option<&str>) -> &'static Encoding {
    let label = match charset {
        Some(label) if !label.is_empty() => label,
        _ => "utf8",
    };
    match Encoding::for_label(label.as_bytes()) {
        Some(encoding) if encoding != REPLACEMENT => encoding,
        _ => WINDOWS_1252,
    }
}

pub(crate) fn decode_charset(charset: Option<&str>, bytes: &[u8]) -> String {
    encoding_for(charset)
        .decode_with_bom_removal(bytes)
        .0
        .into_owned()
}

fn decode_flowed(text: &str, del_sp: bool) -> String {
    let mut lines = text
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    let mut joined = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        if joined.ends_with(' ') && joined != "-- " && !joined.ends_with("\n-- ") {
            if del_sp {
                joined.pop();
            }
        } else {
            joined.push('\n');
        }
        joined.push_str(line);
    }
    let mut out = String::with_capacity(joined.len());
    let mut line_start = true;
    for c in joined.chars() {
        if line_start && c == ' ' {
            line_start = false;
            continue;
        }
        line_start = matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
        out.push(c);
    }
    out
}

fn base64_value(byte: u8) -> u8 {
    match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0,
    }
}

fn decode_base64(clean: &str) -> Vec<u8> {
    let bytes = clean.as_bytes();
    let len = bytes.len();
    let mut length = len.div_ceil(4) * 3;
    match len % 4 {
        3 => length -= 1,
        2 => length -= 2,
        _ if bytes.last() == Some(&b'=') => {
            length -= 1;
            if len >= 2 && bytes[len - 2] == b'=' {
                length -= 1;
            }
        }
        _ => {}
    }
    let mut out = Vec::with_capacity(length);
    for group in bytes.chunks(4) {
        let e = |i: usize| group.get(i).map_or(0, |&b| base64_value(b));
        for byte in [
            (e(0) << 2) | (e(1) >> 4),
            ((e(1) & 15) << 4) | (e(2) >> 2),
            ((e(2) & 3) << 6) | (e(3) & 63),
        ] {
            if out.len() < length {
                out.push(byte);
            }
        }
    }
    out
}

fn is_hex_triplet(part: &[u8]) -> bool {
    part.len() >= 3 && part[0] == b'=' && part[1].is_ascii_hexdigit() && part[2].is_ascii_hexdigit()
}

static SOFT_LINE_BREAK: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"=\r?\n").unwrap());

fn decode_quoted_printable(text: &str) -> Vec<u8> {
    let without_soft_breaks = SOFT_LINE_BREAK.replace_all(text, "");
    let bytes = without_soft_breaks.as_bytes();
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut start = 0;
    for index in 1..bytes.len() {
        if is_hex_triplet(&bytes[index..]) {
            parts.push(&bytes[start..index]);
            start = index;
        }
    }
    parts.push(&bytes[start..]);
    let mut out = Vec::with_capacity(bytes.len());
    for part in parts {
        if part.first() != Some(&b'=') {
            out.extend_from_slice(part);
        } else if is_hex_triplet(part) {
            let hex = std::str::from_utf8(&part[1..3]).unwrap_or("00");
            out.push(u8::from_str_radix(hex, 16).unwrap_or(0));
            out.extend_from_slice(&part[3..]);
        } else if part.len() >= 3 {
            out.extend_from_slice(part);
        }
    }
    out
}

impl Decoder {
    fn update(&mut self, line: &[u8]) {
        match self {
            Decoder::Pass(buffer) => {
                buffer.extend_from_slice(line);
                buffer.push(b'\n');
            }
            Decoder::Base64(clean) => {
                let text = UTF_8.decode_with_bom_removal(line).0;
                clean.extend(
                    text.chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '+' || *c == '/'),
                );
            }
            Decoder::QuotedPrintable { charset, text } => {
                text.push_str(&decode_charset(charset.as_deref(), line));
                text.push('\n');
            }
        }
    }

    fn finish(&mut self) -> Vec<u8> {
        match self {
            Decoder::Pass(buffer) => std::mem::take(buffer),
            Decoder::Base64(clean) => decode_base64(clean),
            Decoder::QuotedPrintable { text, .. } => decode_quoted_printable(text),
        }
    }
}

fn strip_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut escaped = false;
    let mut in_quote = false;
    for c in s.chars() {
        if escaped {
            if depth == 0 {
                out.push(c);
            }
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            if depth == 0 {
                out.push(c);
            }
            continue;
        }
        if c == '"' && depth == 0 {
            in_quote = !in_quote;
            out.push(c);
            continue;
        }
        if !in_quote {
            if c == '(' {
                depth += 1;
                continue;
            }
            if c == ')' && depth > 0 {
                depth -= 1;
                continue;
            }
        }
        if depth == 0 {
            out.push(c);
        }
    }
    out
}

fn decode_percent_with_charset(encoded: &str, charset: Option<&str>) -> String {
    let mut bytes = Vec::with_capacity(encoded.len());
    let raw = encoded.as_bytes();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == b'%'
            && raw.len() > index + 2
            && raw[index + 1].is_ascii_hexdigit()
            && raw[index + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&raw[index + 1..index + 3]).unwrap_or("00");
            bytes.push(u8::from_str_radix(hex, 16).unwrap_or(0));
            index += 3;
        } else {
            bytes.push(raw[index]);
            index += 1;
        }
    }
    decode_charset(Some(charset.unwrap_or("utf-8")), &bytes)
}

fn continuation_suffix(key: &str) -> Option<(usize, u32, bool)> {
    let star = key.rfind('*')?;
    if star + 1 == key.len() {
        let before = &key[..star];
        let digits = before.len() - before.trim_end_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && before[..before.len() - digits].ends_with('*') {
            let number = before[before.len() - digits..].parse().unwrap_or(0);
            return Some((before.len() - digits - 1, number, true));
        }
        return Some((star, 0, true));
    }
    let digits = &key[star + 1..];
    if digits.bytes().all(|b| b.is_ascii_digit()) {
        return Some((star, digits.parse().unwrap_or(0), false));
    }
    None
}

fn decode_continuations(params: &mut BTreeMap<String, String>) {
    struct Pieces {
        charset: Option<String>,
        values: Vec<(u32, String)>,
    }
    let mut grouped: Vec<(String, Pieces)> = Vec::new();
    let keys: Vec<String> = params.keys().cloned().collect();
    for key in keys {
        let Some((cut, number, extended)) = continuation_suffix(&key) else {
            continue;
        };
        let actual = key[..cut].to_lowercase();
        let mut value = params.remove(&key).unwrap_or_default();
        let index = match grouped.iter().position(|(k, _)| *k == actual) {
            Some(index) => index,
            None => {
                grouped.push((
                    actual,
                    Pieces {
                        charset: None,
                        values: Vec::new(),
                    },
                ));
                grouped.len() - 1
            }
        };
        let pieces = &mut grouped[index].1;
        if number == 0
            && extended
            && let Some(first) = value.find('\'')
            && let Some(second) = value[first + 1..].find('\'')
        {
            let charset = &value[..first];
            pieces.charset = Some(if charset.is_empty() {
                "utf-8".to_owned()
            } else {
                charset.to_owned()
            });
            value = value[first + 1 + second + 1..].to_owned();
        }
        pieces.values.push((number, value));
    }
    for (key, mut pieces) in grouped {
        pieces.values.sort_by_key(|(number, _)| *number);
        let joined: String = pieces.values.into_iter().map(|(_, value)| value).collect();
        params.insert(
            key,
            decode_percent_with_charset(&joined, pieces.charset.as_deref()),
        );
    }
}

fn parse_structured(raw: &str) -> Structured {
    let s = strip_comments(raw);
    let mut out = Structured::default();
    let mut key: Option<String> = None;
    let mut value = String::new();
    let mut in_key = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in s.chars() {
        if in_key {
            if c == '=' {
                key = Some(js_trim(&value).to_lowercase());
                in_key = false;
                value.clear();
            } else {
                value.push(c);
            }
            continue;
        }
        if escaped {
            value.push(c);
        } else if c == '\\' {
            escaped = true;
            continue;
        } else if quote == Some(c) {
            quote = None;
        } else if quote.is_none() && c == '"' {
            quote = Some(c);
        } else if quote.is_none() && c == ';' {
            match &key {
                None => {
                    out.value = js_trim(&value).to_owned();
                }
                Some(k) => {
                    out.params.insert(k.clone(), js_trim(&value).to_owned());
                }
            }
            in_key = true;
            value.clear();
        } else {
            value.push(c);
        }
        escaped = false;
    }
    let rest = js_trim(&value).to_owned();
    if !in_key {
        match &key {
            None => {
                out.value = rest;
            }
            Some(k) => {
                out.params.insert(k.clone(), rest);
            }
        }
    } else if !rest.is_empty() {
        out.params.insert(rest.to_lowercase(), String::new());
    }
    out.value = out.value.to_lowercase();
    decode_continuations(&mut out.params);
    out
}

fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if postal_text::is_js_whitespace(c) {
            if !in_ws {
                out.push(' ');
            }
            in_ws = true;
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

struct Tree {
    nodes: Vec<MimeNode>,
    current: usize,
    boundaries: Vec<(Vec<u8>, usize)>,
}

impl Tree {
    fn new() -> Self {
        Self {
            nodes: vec![MimeNode::new(None, 0, "text/plain")],
            current: 0,
            boundaries: Vec::new(),
        }
    }

    fn process_headers(&mut self, id: usize) {
        let node = &mut self.nodes[id];
        let mut lines = std::mem::take(&mut node.header_lines);
        let mut index = lines.len();
        while index > 1 {
            index -= 1;
            if lines[index]
                .chars()
                .next()
                .is_some_and(postal_text::is_js_whitespace)
            {
                let folded = lines.remove(index);
                lines[index - 1].push('\n');
                lines[index - 1].push_str(&folded);
            }
        }
        let mut content_type: Option<String> = None;
        let mut transfer = String::from("8bit");
        let mut disposition = String::new();
        for raw in lines.iter().rev() {
            let normalized = normalize_whitespace(raw);
            let (key, value) = match normalized.find(':') {
                Some(sep) => (
                    js_trim(&normalized[..sep]).to_lowercase(),
                    js_trim(&normalized[sep + 1..]).to_owned(),
                ),
                None => (js_trim(&normalized).to_lowercase(), String::new()),
            };
            match key.as_str() {
                "content-type" => {
                    content_type.get_or_insert(value);
                }
                "content-transfer-encoding" => transfer = value,
                "content-disposition" => disposition = value,
                "content-id" => node.content_id = Some(value),
                _ => {}
            }
        }
        node.content_type = parse_structured(content_type.as_deref().unwrap_or(node.default_type));
        node.multipart = node
            .content_type
            .value
            .strip_prefix("multipart/")
            .map(str::to_owned);
        if node.multipart.is_some()
            && let Some(boundary) = node
                .content_type
                .params
                .get("boundary")
                .filter(|b| !b.is_empty())
        {
            self.boundaries.push((boundary.as_bytes().to_vec(), id));
        }
        let node = &mut self.nodes[id];
        node.disposition = parse_structured(&disposition);
        let encoding = transfer.to_lowercase();
        let encoding = encoding
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .next()
            .unwrap_or_default();
        node.decoder = if encoding.contains("base64") {
            Decoder::Base64(String::new())
        } else if encoding.contains("quoted-printable") {
            Decoder::QuotedPrintable {
                charset: node.charset().map(str::to_owned),
                text: String::new(),
            }
        } else {
            Decoder::Pass(Vec::new())
        };
    }

    fn finalize(&mut self, id: usize) {
        if self.nodes[id].state == State::Finished {
            return;
        }
        if self.nodes[id].state == State::Header {
            self.process_headers(id);
        }
        if let Some(position) = self.boundaries.iter().rposition(|(_, node)| *node == id) {
            self.boundaries.remove(position);
        }
        self.finalize_children(id);
        let node = &mut self.nodes[id];
        node.content = Some(node.decoder.finish());
        node.state = State::Finished;
    }

    fn finalize_children(&mut self, id: usize) {
        for child in self.nodes[id].children.clone() {
            self.finalize(child);
        }
    }

    fn feed(&mut self, line: &[u8]) -> Result<(), BodyError> {
        let id = self.current;
        let node = &mut self.nodes[id];
        match node.state {
            State::Header => {
                if line.is_empty() {
                    node.state = State::Body;
                    self.process_headers(id);
                    return Ok(());
                }
                node.header_size += line.len();
                if node.header_size > MAX_HEADERS_SIZE {
                    return Err(BodyError::HeadersTooLarge);
                }
                node.header_lines
                    .push(UTF_8.decode_with_bom_removal(line).0.into_owned());
            }
            State::Body => node.decoder.update(line),
            State::Finished => {}
        }
        Ok(())
    }

    fn boundary_at(&self, line: &[u8]) -> Option<(usize, bool)> {
        if self.boundaries.is_empty() || line.len() <= 2 || line[0] != b'-' || line[1] != b'-' {
            return None;
        }
        for (boundary, node) in self.boundaries.iter().rev() {
            if line.len() < boundary.len() + 2
                || &line[2..boundary.len() + 2] != boundary.as_slice()
            {
                continue;
            }
            let terminator = line.len() >= boundary.len() + 4
                && &line[boundary.len() + 2..boundary.len() + 4] == b"--";
            let end = boundary.len() + if terminator { 4 } else { 2 };
            if line[end..].iter().all(|&b| b == b' ' || b == b'\t') {
                return Some((*node, terminator));
            }
        }
        None
    }

    fn process_line(&mut self, line: &[u8], last: bool) -> Result<(), BodyError> {
        if let Some((node, terminator)) = self.boundary_at(line) {
            if terminator {
                self.finalize(node);
                self.current = self.nodes[node].parent.unwrap_or(0);
            } else {
                self.finalize_children(node);
                let depth = self.nodes[node].depth + 1;
                if depth > MAX_MIME_DEPTH {
                    return Err(BodyError::TooDeep);
                }
                let default_type = if self.nodes[node].multipart.as_deref() == Some("digest") {
                    "message/rfc822"
                } else {
                    "text/plain"
                };
                self.nodes
                    .push(MimeNode::new(Some(node), depth, default_type));
                let child = self.nodes.len() - 1;
                self.nodes[node].children.push(child);
                self.current = child;
            }
        } else {
            self.feed(line)?;
        }
        if last {
            self.finalize(0);
        }
        Ok(())
    }
}

fn read_lines(raw: &[u8]) -> impl Iterator<Item = (&[u8], bool)> {
    let mut position = 0;
    std::iter::from_fn(move || {
        if position >= raw.len() {
            return None;
        }
        let start = position;
        let mut end = position;
        while position < raw.len() {
            let byte = raw[position];
            position += 1;
            if byte != b'\r' && byte != b'\n' {
                end = position;
            }
            if byte == b'\n' {
                break;
            }
        }
        Some((&raw[start..end], position >= raw.len()))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextType {
    Plain,
    Html,
}

#[derive(Clone)]
enum Item {
    Text(String),
    Message(Rc<SubMessage>),
}

#[derive(Default, Clone)]
struct Entry {
    plain: Option<Vec<Item>>,
    html: Option<Vec<Item>>,
}

impl Entry {
    fn get(&self, kind: TextType) -> Option<&Vec<Item>> {
        match kind {
            TextType::Plain => self.plain.as_ref(),
            TextType::Html => self.html.as_ref(),
        }
    }

    fn push(&mut self, kind: TextType, item: Item) {
        let list = match kind {
            TextType::Plain => &mut self.plain,
            TextType::Html => &mut self.html,
        };
        list.get_or_insert_with(Vec::new).push(item);
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum EntryKey {
    Node(usize, usize),
}

#[derive(Default)]
struct Collected {
    entries: Vec<(EntryKey, Entry)>,
    types: Vec<TextType>,
    inline: Vec<(String, InlinePart)>,
}

impl Collected {
    fn entry(&mut self, key: EntryKey) -> &mut Entry {
        let index = match self.entries.iter().position(|(k, _)| *k == key) {
            Some(index) => index,
            None => {
                self.entries.push((key, Entry::default()));
                self.entries.len() - 1
            }
        };
        &mut self.entries[index].1
    }

    fn add_type(&mut self, kind: TextType) {
        if !self.types.contains(&kind) {
            self.types.push(kind);
        }
    }
}

#[derive(Clone, Copy)]
struct Position {
    id: usize,
    alternative: Option<usize>,
    related: bool,
    force_attachments: bool,
    scope: usize,
}

struct Parse<'a> {
    options: BodyOptions,
    embedded: usize,
    next_scope: &'a mut usize,
}

fn mailboxes(address: Option<&Address<'_>>) -> Vec<Mailbox> {
    let single = |addr: &mail_parser::Addr<'_>| Mailbox::Single {
        name: addr.name.as_deref().unwrap_or_default().to_owned(),
        address: addr.address.as_deref().map(str::to_owned),
    };
    match address {
        None => Vec::new(),
        Some(Address::List(list)) => list.iter().map(single).collect(),
        Some(Address::Group(groups)) => {
            let mut out = Vec::new();
            for group in groups {
                match &group.name {
                    Some(name) => out.push(Mailbox::Group {
                        name: name.to_string(),
                        members: group.addresses.iter().map(single).collect(),
                    }),
                    None => out.extend(group.addresses.iter().map(single)),
                }
            }
            out
        }
    }
}

fn sub_message_headers(content: &[u8], html: Option<String>, text: Option<String>) -> SubMessage {
    let parsed = MessageParser::default().parse_headers(content);
    let Some(message) = parsed else {
        return SubMessage {
            from: None,
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: None,
            date: None,
            html,
            text,
        };
    };
    let date = message.header_raw("Date").map(|_| match message.date() {
        Some(date) if date.is_valid() => SubDate::Valid(date.to_timestamp()),
        _ => SubDate::Invalid,
    });
    SubMessage {
        from: mailboxes(message.from()).into_iter().next(),
        to: mailboxes(message.to()),
        cc: mailboxes(message.cc()),
        bcc: mailboxes(message.bcc()),
        subject: message.subject().map(str::to_owned),
        date,
        html,
        text,
    }
}

impl Parse<'_> {
    fn run(&mut self, raw: &[u8]) -> Result<(Collected, ParsedTexts), BodyError> {
        let mut tree = Tree::new();
        for (line, last) in read_lines(raw) {
            tree.process_line(line, last)?;
        }
        let force_attachments = tree.nodes.iter().any(|node| {
            node.multipart.is_none()
                && matches!(
                    node.content_type.value.as_str(),
                    "message/delivery-status" | "message/feedback-report"
                )
        });
        let scope = *self.next_scope;
        *self.next_scope += 1;
        let mut collected = Collected::default();
        let root = Position {
            id: 0,
            alternative: None,
            related: false,
            force_attachments,
            scope,
        };
        let mut stack = vec![root];
        while let Some(position) = stack.pop() {
            let node = &tree.nodes[position.id];
            let mut inherited = position;
            if node.multipart.is_none() {
                self.visit_leaf(&tree, position, &mut collected)?;
            } else if node.multipart.as_deref() == Some("alternative") {
                inherited.alternative = Some(position.id);
            } else if node.multipart.as_deref() == Some("related") {
                inherited.related = true;
            }
            for &child in node.children.iter().rev() {
                stack.push(Position {
                    id: child,
                    ..inherited
                });
            }
        }
        let texts = assemble(&collected, self.options)?;
        Ok((collected, texts))
    }

    fn visit_leaf(
        &mut self,
        tree: &Tree,
        position: Position,
        collected: &mut Collected,
    ) -> Result<(), BodyError> {
        let Position {
            id,
            alternative,
            related,
            force_attachments,
            scope,
        } = position;
        let node = &tree.nodes[id];
        let kind = node.content_type.value.as_str();
        let disposition = node.disposition.value.as_str();
        let inline_message =
            kind == "message/rfc822" && (disposition.is_empty() || disposition == "inline");
        if inline_message && !force_attachments {
            if self.embedded >= MAX_EMBEDDED_MESSAGES {
                return Err(BodyError::TooDeep);
            }
            let content = node.content.clone().unwrap_or_default();
            let mut nested = Parse {
                options: self.options,
                embedded: self.embedded + 1,
                next_scope: &mut *self.next_scope,
            };
            let (sub, texts) = nested.run(&content)?;
            let message = Rc::new(sub_message_headers(
                &content,
                texts.html.clone(),
                texts.text.clone(),
            ));
            let key = EntryKey::Node(scope, id);
            collected.entry(key);
            if message.text.is_some() || message.html.is_none() {
                collected
                    .entry(key)
                    .push(TextType::Plain, Item::Message(message.clone()));
                collected.add_type(TextType::Plain);
            }
            if message.html.is_some() {
                collected
                    .entry(key)
                    .push(TextType::Html, Item::Message(message.clone()));
                collected.add_type(TextType::Html);
            }
            collected.entries.extend(sub.entries);
            collected.inline.extend(sub.inline);
            return Ok(());
        }
        let text_kind = match kind {
            "text/html" => Some(TextType::Html),
            "text/plain" => Some(TextType::Plain),
            _ => None,
        };
        if let Some(text_kind) = text_kind.filter(|_| disposition != "attachment") {
            let selector = alternative.unwrap_or(id);
            let text = node.text_content();
            collected
                .entry(EntryKey::Node(scope, selector))
                .push(text_kind, Item::Text(text));
            collected.add_type(text_kind);
            return Ok(());
        }
        let calendar = matches!(kind, "text/calendar" | "application/ics");
        if related
            && !calendar
            && let (Some(content_id), Some(content)) = (&node.content_id, &node.content)
            && !content_id.is_empty()
        {
            let cid = content_id.strip_prefix('<').unwrap_or(content_id);
            let cid = cid.strip_suffix('>').unwrap_or(cid);
            collected.inline.push((
                cid.to_owned(),
                InlinePart {
                    mime_type: kind.to_owned(),
                    data: content.clone(),
                },
            ));
        }
        Ok(())
    }
}

#[derive(Default)]
struct ParsedTexts {
    html: Option<String>,
    text: Option<String>,
}

fn header_error(_: HeaderError) -> BodyError {
    BodyError::EmbeddedHeader
}

fn assemble(collected: &Collected, options: BodyOptions) -> Result<ParsedTexts, BodyError> {
    let mut plain: Vec<String> = Vec::new();
    let mut html: Vec<String> = Vec::new();
    for (_, entry) in &collected.entries {
        for &kind in &collected.types {
            let out = match kind {
                TextType::Plain => &mut plain,
                TextType::Html => &mut html,
            };
            let (items, convert) = match entry.get(kind) {
                Some(items) => (items.as_slice(), false),
                None => {
                    let other = if kind == TextType::Html {
                        TextType::Plain
                    } else {
                        TextType::Html
                    };
                    (entry.get(other).map_or(&[][..], Vec::as_slice), true)
                }
            };
            for item in items {
                out.push(match (item, kind) {
                    (Item::Text(value), TextType::Html) if convert => text_to_html(value),
                    (Item::Text(value), TextType::Plain) if convert => html_to_text(value),
                    (Item::Text(value), _) => value.clone(),
                    (Item::Message(message), TextType::Html) => {
                        format_html_header(message, options.utc_offset_seconds)
                            .map_err(header_error)?
                    }
                    (Item::Message(message), TextType::Plain) => {
                        format_text_header(message, options.utc_offset_seconds)
                            .map_err(header_error)?
                    }
                });
            }
        }
    }
    let joined = |kind: TextType, parts: Vec<String>| {
        collected
            .types
            .contains(&kind)
            .then(|| parts.join("\n"))
            .filter(|s| !s.is_empty())
    };
    Ok(ParsedTexts {
        html: joined(TextType::Html, html),
        text: joined(TextType::Plain, plain),
    })
}

pub fn parse_body(raw: &[u8], options: BodyOptions) -> Result<ParsedBody, BodyError> {
    let mut next_scope = 0;
    let (collected, texts) = Parse {
        options,
        embedded: 0,
        next_scope: &mut next_scope,
    }
    .run(raw)?;
    let mut inline = InlineParts::new();
    for (cid, part) in collected.inline {
        inline.insert(cid, part);
    }
    Ok(ParsedBody {
        html: texts.html,
        text: texts.text,
        inline,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_headers_keep_params_and_lowercase_the_value() {
        let parsed =
            parse_structured("Text/HTML; Charset=\"UTF-8\" (comment); format=flowed; flag");
        assert_eq!(parsed.value, "text/html");
        assert_eq!(
            parsed.params.get("charset").map(String::as_str),
            Some("UTF-8")
        );
        assert_eq!(
            parsed.params.get("format").map(String::as_str),
            Some("flowed")
        );
        assert_eq!(parsed.params.get("flag").map(String::as_str), Some(""));
    }

    #[test]
    fn rfc2231_continuations_are_joined_and_decoded() {
        let parsed = parse_structured("attachment; filename*0*=utf-8''%D0%9E; filename*1=.pdf");
        assert_eq!(
            parsed.params.get("filename").map(String::as_str),
            Some("О.pdf")
        );
    }

    #[test]
    fn base64_keeps_the_postal_padding_rules() {
        assert_eq!(decode_base64("SGVsbG8"), b"Hello");
        assert_eq!(decode_base64("QQ"), b"A");
    }

    #[test]
    fn quoted_printable_keeps_invalid_sequences() {
        assert_eq!(decode_quoted_printable("a=3Db =zz =4\n"), b"a=b =zz =4\n");
    }

    #[test]
    fn lines_drop_trailing_carriage_returns_only() {
        let lines: Vec<&[u8]> = read_lines(b"a\rb\r\r\nc").map(|(line, _)| line).collect();
        assert_eq!(lines, vec![&b"a\rb"[..], &b"c"[..]]);
    }

    #[test]
    fn deep_multipart_nesting_is_refused() {
        let mut raw = String::new();
        for depth in 0..300 {
            raw.push_str(&format!(
                "Content-Type: multipart/mixed; boundary=\"b{depth}\"\r\n\r\n--b{depth}\r\n"
            ));
        }
        assert_eq!(
            parse_body(raw.as_bytes(), BodyOptions::default()),
            Err(BodyError::TooDeep)
        );
    }
}
