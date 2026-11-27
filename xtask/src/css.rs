#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub selector: String,
    pub decls: Vec<(String, String)>,
}

pub fn parse(source: &str) -> Vec<Rule> {
    let mut rules = Vec::new();
    collect(&strip_comments(source), &mut rules);
    rules
}

fn collect(source: &str, rules: &mut Vec<Rule>) {
    let mut rest = source;
    loop {
        let Some(open) = rest.find('{') else { return };
        let head = rest[..open].trim();
        let Some(close) = matching_brace(rest, open) else {
            return;
        };
        let body = &rest[open + 1..close];
        let selector = head.rsplit(';').next().unwrap_or(head).trim();
        if selector.starts_with("@layer") || selector.starts_with("@media") {
            collect(body, rules);
        } else {
            rules.push(Rule {
                selector: selector.to_string(),
                decls: declarations(body),
            });
        }
        rest = &rest[close + 1..];
    }
}

fn matching_brace(source: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in source[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn strip_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn declarations(body: &str) -> Vec<(String, String)> {
    split_top(body, ';')
        .into_iter()
        .filter_map(|decl| {
            let (name, value) = decl.split_once(':')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || name.contains('{') || value.is_empty() {
                return None;
            }
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

pub fn split_top(source: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, c) in source.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '{') => depth += 1,
            (None, ')' | '}') => depth -= 1,
            (None, c) if c == separator && depth == 0 => {
                parts.push(source[start..i].trim());
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    let tail = source[start..].trim();
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_rules_and_nested_layers() {
        let rules = parse(
            "@import 'x.css';\n:root { --a: #fff; --b: var(--a); }\n/* note */\n@layer components {\n.t-x { font-size: var(--a); color: red }\n}\n[data-theme='dark'] { color-scheme: dark; --a: #000; }",
        );
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].selector, ":root");
        assert_eq!(
            rules[0].decls,
            vec![
                ("--a".to_string(), "#fff".to_string()),
                ("--b".to_string(), "var(--a)".to_string())
            ]
        );
        assert_eq!(rules[1].selector, ".t-x");
        assert_eq!(rules[1].decls.len(), 2);
        assert_eq!(rules[2].selector, "[data-theme='dark']");
        assert_eq!(rules[2].decls[1], ("--a".to_string(), "#000".to_string()));
    }

    #[test]
    fn keeps_values_with_colons_and_semicolons_in_parens() {
        let rules = parse(
            "@font-face{font-family:\"A B\";src:url(\"/f.woff2\") format(\"woff2\");size-adjust:106%;}",
        );
        assert_eq!(rules[0].selector, "@font-face");
        assert_eq!(rules[0].decls[1].1, "url(\"/f.woff2\") format(\"woff2\")");
        assert_eq!(rules[0].decls[2].1, "106%");
    }

    #[test]
    fn splits_only_at_top_level() {
        assert_eq!(
            split_top("0 1px rgba(0, 0, 0, 0.3), 0 2px red", ','),
            vec!["0 1px rgba(0, 0, 0, 0.3)", "0 2px red"]
        );
    }
}
