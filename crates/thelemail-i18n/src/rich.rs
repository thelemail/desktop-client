use std::fmt::{self, Display};

pub type Args<'a> = &'a [(&'a str, &'a dyn Display)];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub tag: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rich {
    runs: Vec<Run>,
}

impl Rich {
    pub fn parse(source: &'static str) -> Self {
        Self::template(source, &[])
    }

    pub fn template(source: &'static str, args: Args) -> Self {
        let runs = segments(source)
            .into_iter()
            .map(|(text, tag)| Run {
                text: fill(text, args),
                tag,
            })
            .collect();
        Self { runs }
    }

    pub fn runs(&self) -> &[Run] {
        &self.runs
    }

    pub fn plain(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

impl Display for Rich {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.runs.iter().try_for_each(|run| f.write_str(&run.text))
    }
}

pub fn fill(template: &str, args: Args) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            args.iter()
                .find(|(n, _)| *n == name)
                .map(|(_, v)| (close, v.to_string()))
        });
        match value {
            Some((close, value)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn has_tags(source: &str) -> bool {
    segments(source).iter().any(|(_, tag)| tag.is_some())
}

fn segments(source: &str) -> Vec<(&str, Option<&str>)> {
    let mut out = Vec::new();
    let mut last = 0;
    let mut at = 0;
    while let Some(offset) = source[at..].find('<') {
        let start = at + offset;
        match tagged(source, start) {
            Some((name, inner, end)) => {
                if start > last {
                    out.push((&source[last..start], None));
                }
                out.push((inner, Some(name)));
                last = end;
                at = end;
            }
            None => at = start + 1,
        }
    }
    if last < source.len() {
        out.push((&source[last..], None));
    }
    out
}

fn tagged(source: &str, start: usize) -> Option<(&str, &str, usize)> {
    let rest = &source[start + 1..];
    let name_len = rest
        .bytes()
        .enumerate()
        .take_while(|(i, b)| b.is_ascii_lowercase() || (*i > 0 && b.is_ascii_digit()))
        .count();
    if name_len == 0 || rest.as_bytes().get(name_len) != Some(&b'>') {
        return None;
    }
    let name = &rest[..name_len];
    let body = &rest[name_len + 1..];
    let close = format!("</{name}>");
    let inner_len = body.find(&close)?;
    let end = start + 1 + name_len + 1 + inner_len + close.len();
    Some((name, &body[..inner_len], end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(rich: &Rich) -> Vec<(&str, Option<&str>)> {
        rich.runs()
            .iter()
            .map(|run| (run.text.as_str(), run.tag))
            .collect()
    }

    #[test]
    fn splits_tagged_spans_out_of_a_sentence() {
        let rich = Rich::parse("Read the <link>guide</link> first.");
        assert_eq!(
            runs(&rich),
            [
                ("Read the ", None),
                ("guide", Some("link")),
                (" first.", None)
            ]
        );
    }

    #[test]
    fn handles_several_tags_and_plain_text() {
        let rich = Rich::parse("<b>Note:</b> see <link>docs</link>");
        assert_eq!(
            runs(&rich),
            [
                ("Note:", Some("b")),
                (" see ", None),
                ("docs", Some("link"))
            ]
        );
        assert_eq!(runs(&Rich::parse("plain")), [("plain", None)]);
    }

    #[test]
    fn leaves_unmatched_markup_as_text() {
        assert_eq!(runs(&Rich::parse("a <b>b</i> c")), [("a <b>b</i> c", None)]);
        assert_eq!(runs(&Rich::parse("x <b>y")), [("x <b>y", None)]);
        assert_eq!(runs(&Rich::parse("1 < 2 <3>")), [("1 < 2 <3>", None)]);
        assert!(!has_tags("<p>open"));
    }

    #[test]
    fn does_not_nest_and_keeps_inner_markup() {
        let rich = Rich::parse("<b>a <i>b</i></b> <br>c");
        assert_eq!(runs(&rich), [("a <i>b</i>", Some("b")), (" <br>c", None)]);
    }

    #[test]
    fn keeps_empty_tags_and_digits_in_names() {
        let rich = Rich::parse("Sign in as <email></email>. <h2>x</h2>");
        assert_eq!(
            runs(&rich),
            [
                ("Sign in as ", None),
                ("", Some("email")),
                (". ", None),
                ("x", Some("h2"))
            ]
        );
    }

    #[test]
    fn fills_placeholders_without_parsing_their_values() {
        let domain = "<b>evil</b>.example";
        let count = 3;
        let rich = Rich::template(
            "Records for <mono>{domain}</mono> and {count} more",
            &[("domain", &domain), ("count", &count)],
        );
        assert_eq!(
            runs(&rich),
            [
                ("Records for ", None),
                ("<b>evil</b>.example", Some("mono")),
                (" and 3 more", None)
            ]
        );
        assert_eq!(rich.plain(), "Records for <b>evil</b>.example and 3 more");
        assert_eq!(rich.to_string(), rich.plain());
    }

    #[test]
    fn fill_leaves_unknown_and_unclosed_braces() {
        let name = "Ana";
        assert_eq!(
            fill("{name} {other} {", &[("name", &name)]),
            "Ana {other} {"
        );
        assert_eq!(fill("{name}{name}", &[("name", &"{name}")]), "{name}{name}");
    }
}
