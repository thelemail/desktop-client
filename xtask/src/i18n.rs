use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

pub const LOCALES: [&str; 4] = ["en", "de", "fr", "pt"];
pub const WEB_DIR: &str = "packages/core/messages";
pub const DESKTOP_DIR: &str = "messages";
pub const MESSAGES_RS: &str = "crates/thelemail-i18n/src/messages.rs";

pub struct Catalog {
    pub origin: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
enum Text {
    Plain(String),
    Variant(Variant),
}

#[derive(Debug, Clone)]
struct Variant {
    inputs: Vec<String>,
    locals: BTreeMap<String, Local>,
    selector: String,
    arms: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct Local {
    input: String,
    ordinal: bool,
}

struct Message {
    key: String,
    texts: Vec<Text>,
}

#[derive(PartialEq, Eq)]
enum Kind {
    Static,
    Owned,
    Rich,
}

struct Input {
    name: String,
    numeric: bool,
}

pub fn keys(catalog: &Catalog) -> Result<BTreeSet<String>, String> {
    let parsed = parse_catalog(catalog)?;
    Ok(parsed.into_keys().collect())
}

pub fn generate(tag: &str, catalogs: &[Catalog]) -> Result<String, String> {
    let mut messages: BTreeMap<String, Message> = BTreeMap::new();
    for catalog in catalogs {
        for (key, texts) in parse_catalog(catalog)? {
            if messages.contains_key(&key) {
                return Err(format!(
                    "{key} in {} is already defined elsewhere",
                    catalog.origin
                ));
            }
            messages.insert(key.clone(), Message { key, texts });
        }
    }
    let mut out = String::new();
    writeln!(out, "use crate::plural::{{Category, cardinal, ordinal}};").unwrap();
    writeln!(out, "use crate::rich::{{Rich, fill}};").unwrap();
    writeln!(out, "use crate::{{Locale, locale}};").unwrap();
    writeln!(out, "pub const SOURCE_TAG: &str = {tag:?};").unwrap();
    writeln!(out, "pub const KEY_COUNT: usize = {};", messages.len()).unwrap();
    for message in messages.values() {
        emit(&mut out, message).map_err(|e| format!("{}: {e}", message.key))?;
    }
    Ok(out)
}

pub fn unused(keys: &BTreeSet<String>, root: &Path) -> Result<Vec<String>, String> {
    let mut sources = String::new();
    for dir in ["app", "crates"] {
        collect_sources(&root.join(dir), &root.join(MESSAGES_RS), &mut sources)?;
    }
    Ok(keys
        .iter()
        .filter(|key| !used(&sources, key))
        .cloned()
        .collect())
}

fn used(sources: &str, key: &str) -> bool {
    let needle = format!("m::{key}");
    sources.match_indices(&needle).any(|(at, _)| {
        sources[at + needle.len()..]
            .chars()
            .next()
            .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
    })
}

fn collect_sources(dir: &Path, skip: &Path, out: &mut String) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("reading {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            collect_sources(&path, skip, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") && path != skip {
            out.push_str(
                &std::fs::read_to_string(&path)
                    .map_err(|e| format!("reading {}: {e}", path.display()))?,
            );
            out.push('\n');
        }
    }
    Ok(())
}

fn parse_catalog(catalog: &Catalog) -> Result<BTreeMap<String, Vec<Text>>, String> {
    let mut per_locale = Vec::new();
    for (locale, raw) in LOCALES.iter().zip(&catalog.files) {
        let file = parse_file(raw).map_err(|e| format!("{}/{locale}.json: {e}", catalog.origin))?;
        per_locale.push(file);
    }
    let all: BTreeSet<&String> = per_locale.iter().flat_map(|f| f.keys()).collect();
    let mut problems = Vec::new();
    for (locale, file) in LOCALES.iter().zip(&per_locale) {
        for key in &all {
            if !file.contains_key(*key) {
                problems.push(format!(
                    "{key} is missing in {}/{locale}.json",
                    catalog.origin
                ));
            }
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    let mut out = BTreeMap::new();
    for key in all {
        let texts = per_locale.iter().map(|f| f[key].clone()).collect();
        out.insert(key.clone(), texts);
    }
    Ok(out)
}

fn parse_file(raw: &str) -> Result<BTreeMap<String, Text>, String> {
    let value: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    let object = value.as_object().ok_or("not a JSON object")?;
    let mut out = BTreeMap::new();
    for (key, value) in object {
        if key.starts_with('$') {
            continue;
        }
        if !valid_key(key) {
            return Err(format!("{key} is not a valid message key"));
        }
        let text = match value {
            Value::String(s) => Text::Plain(s.clone()),
            Value::Array(items) => {
                let [item] = items.as_slice() else {
                    return Err(format!("{key} must have exactly one variant entry"));
                };
                Text::Variant(parse_variant(item).map_err(|e| format!("{key}: {e}"))?)
            }
            _ => return Err(format!("{key} is neither a string nor a variant list")),
        };
        out.insert(key.clone(), text);
    }
    Ok(out)
}

fn valid_key(key: &str) -> bool {
    key.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn parse_variant(item: &Value) -> Result<Variant, String> {
    let strings = |field: &str| -> Result<Vec<String>, String> {
        item[field]
            .as_array()
            .ok_or_else(|| format!("{field} is not a list"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or(format!("{field} holds a non-string"))
            })
            .collect()
    };
    let mut inputs = Vec::new();
    let mut locals = BTreeMap::new();
    for declaration in strings("declarations")? {
        let words: Vec<&str> = declaration.split_whitespace().collect();
        match words.as_slice() {
            ["input", name] => inputs.push((*name).to_string()),
            ["local", name, "=", source, "plural"] | ["local", name, "=", source, "plural", _]
                if source.ends_with(':') =>
            {
                let ordinal = match words.get(5) {
                    None => false,
                    Some(&"type=ordinal") => true,
                    Some(other) => return Err(format!("unsupported plural option {other}")),
                };
                let input = source.trim_end_matches(':').to_string();
                locals.insert((*name).to_string(), Local { input, ordinal });
            }
            _ => return Err(format!("unsupported declaration {declaration:?}")),
        }
    }
    for local in locals.values() {
        if !inputs.contains(&local.input) {
            return Err(format!("local reads undeclared input {}", local.input));
        }
    }
    let selectors = strings("selectors")?;
    let [selector] = selectors.as_slice() else {
        return Err("exactly one selector is supported".to_string());
    };
    if !inputs.contains(selector) && !locals.contains_key(selector) {
        return Err(format!("selector {selector} is not declared"));
    }
    let arms_object = item["match"].as_object().ok_or("match is not an object")?;
    let mut arms = Vec::new();
    for (condition, text) in arms_object {
        let (name, value) = condition
            .split_once('=')
            .ok_or_else(|| format!("bad match key {condition}"))?;
        if name.trim() != selector {
            return Err(format!(
                "match key {condition} does not use selector {selector}"
            ));
        }
        let text = text.as_str().ok_or("match value is not a string")?;
        arms.push((value.trim().to_string(), text.to_string()));
    }
    if !arms.iter().any(|(v, _)| v == "*") {
        return Err("no * variant".to_string());
    }
    Ok(Variant {
        inputs,
        locals,
        selector: selector.clone(),
        arms,
    })
}

fn placeholders(text: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['{', '}']) {
        if rest[open..].starts_with('}') {
            return Err(format!("stray }} in {text:?}"));
        }
        let after = &rest[open + 1..];
        let close = after
            .find('}')
            .ok_or_else(|| format!("unclosed {{ in {text:?}"))?;
        let name = &after[..close];
        let mut chars = name.chars();
        let valid = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(format!("bad placeholder {{{name}}} in {text:?}"));
        }
        if !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
        rest = &after[close + 1..];
    }
    Ok(out)
}

fn all_texts(text: &Text) -> Vec<&str> {
    match text {
        Text::Plain(s) => vec![s.as_str()],
        Text::Variant(v) => v.arms.iter().map(|(_, t)| t.as_str()).collect(),
    }
}

fn inputs_of(message: &Message) -> Result<Vec<Input>, String> {
    let mut names: Vec<String> = Vec::new();
    let mut numeric = BTreeSet::new();
    let mut declared: Option<Vec<String>> = None;
    for text in &message.texts {
        if let Text::Variant(v) = text {
            let mut sorted = v.inputs.clone();
            sorted.sort();
            match &declared {
                None => declared = Some(sorted),
                Some(previous) if *previous != sorted => {
                    return Err("locales declare different inputs".to_string());
                }
                Some(_) => {}
            }
            for name in &v.inputs {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
            numeric.extend(v.locals.values().map(|l| l.input.clone()));
        }
    }
    let base: Vec<String> = all_texts(&message.texts[0])
        .into_iter()
        .map(placeholders)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    for (locale, text) in LOCALES.iter().zip(&message.texts) {
        for body in all_texts(text) {
            for name in placeholders(body)? {
                let known = names.contains(&name) || (declared.is_none() && base.contains(&name));
                if !known {
                    return Err(format!("{locale} uses undeclared placeholder {{{name}}}"));
                }
            }
        }
    }
    if declared.is_none() {
        for name in base {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    let mut idents = BTreeSet::new();
    for name in &names {
        if !idents.insert(ident(name)) {
            return Err(format!("input {name} collides with another input"));
        }
    }
    Ok(names
        .into_iter()
        .map(|name| Input {
            numeric: numeric.contains(&name),
            name,
        })
        .collect())
}

fn ident(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    const KEYWORDS: &[&str] = &[
        "as", "async", "await", "break", "const", "continue", "crate", "do", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod",
        "move", "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait",
        "true", "try", "type", "unsafe", "use", "where", "while", "yield",
    ];
    if KEYWORDS.contains(&out.as_str()) {
        format!("r#{out}")
    } else {
        out
    }
}

fn has_tags(text: &str) -> bool {
    let mut at = 0;
    while let Some(offset) = text[at..].find('<') {
        let start = at + offset;
        let rest = &text[start + 1..];
        let len = rest
            .bytes()
            .enumerate()
            .take_while(|(i, b)| b.is_ascii_lowercase() || (*i > 0 && b.is_ascii_digit()))
            .count();
        if len > 0 && rest.as_bytes().get(len) == Some(&b'>') {
            let close = format!("</{}>", &rest[..len]);
            if rest[len + 1..].contains(&close) {
                return true;
            }
        }
        at = start + 1;
    }
    false
}

fn locale_variant(locale: &str) -> &'static str {
    match locale {
        "en" => "Locale::En",
        "de" => "Locale::De",
        "fr" => "Locale::Fr",
        _ => "Locale::Pt",
    }
}

fn category(value: &str) -> Result<&'static str, String> {
    Ok(match value {
        "zero" => "Category::Zero",
        "one" => "Category::One",
        "two" => "Category::Two",
        "few" => "Category::Few",
        "many" => "Category::Many",
        "other" => "Category::Other",
        other => return Err(format!("unknown plural category {other}")),
    })
}

fn emit(out: &mut String, message: &Message) -> Result<(), String> {
    let inputs = inputs_of(message)?;
    let rich =
        !message.key.ends_with("_html") && message.texts.iter().flat_map(all_texts).any(has_tags);
    let kind = if rich {
        Kind::Rich
    } else if inputs.is_empty() {
        Kind::Static
    } else {
        Kind::Owned
    };
    let params: Vec<String> = inputs
        .iter()
        .map(|input| {
            let ty = if input.numeric {
                "u64"
            } else {
                "impl std::fmt::Display"
            };
            format!("{}: {ty}", ident(&input.name))
        })
        .collect();
    let ret = match kind {
        Kind::Static => "&'static str",
        Kind::Owned => "String",
        Kind::Rich => "Rich",
    };
    writeln!(
        out,
        "pub fn {}({}) -> {ret} {{",
        message.key,
        params.join(", ")
    )
    .unwrap();
    if !inputs.is_empty() {
        let args: Vec<String> = inputs
            .iter()
            .map(|input| format!("({:?}, &{})", input.name, ident(&input.name)))
            .collect();
        writeln!(
            out,
            "let args: &[(&str, &dyn std::fmt::Display)] = &[{}];",
            args.join(", ")
        )
        .unwrap();
    }
    let body = |text: &str| -> String {
        match kind {
            Kind::Static => format!("{text:?}"),
            Kind::Owned => format!("fill({text:?}, args)"),
            Kind::Rich if inputs.is_empty() => format!("Rich::parse({text:?})"),
            Kind::Rich => format!("Rich::template({text:?}, args)"),
        }
    };
    writeln!(out, "match locale() {{").unwrap();
    for (locale, text) in LOCALES.iter().zip(&message.texts) {
        let arm = match text {
            Text::Plain(s) => body(s),
            Text::Variant(v) => select(v, &inputs, locale, &body)?,
        };
        writeln!(out, "{} => {arm},", locale_variant(locale)).unwrap();
    }
    writeln!(out, "}}\n}}").unwrap();
    Ok(())
}

fn select(
    variant: &Variant,
    inputs: &[Input],
    locale: &str,
    body: &dyn Fn(&str) -> String,
) -> Result<String, String> {
    let mut arms: Vec<(String, String)> = Vec::new();
    let mut fallback = None;
    let scrutinee;
    if let Some(local) = variant.locals.get(&variant.selector) {
        let rule = if local.ordinal { "ordinal" } else { "cardinal" };
        scrutinee = format!(
            "{rule}({}, {})",
            locale_variant(locale),
            ident(&local.input)
        );
        for (value, text) in &variant.arms {
            if value == "*" {
                fallback = Some(body(text));
            } else {
                arms.push((category(value)?.to_string(), body(text)));
            }
        }
    } else {
        let input = inputs
            .iter()
            .find(|i| i.name == variant.selector)
            .ok_or("selector input missing")?;
        scrutinee = if input.numeric {
            ident(&input.name)
        } else {
            format!("{}.to_string().as_str()", ident(&input.name))
        };
        for (value, text) in &variant.arms {
            if value == "*" {
                fallback = Some(body(text));
            } else if input.numeric {
                let n: u64 = value
                    .parse()
                    .map_err(|_| format!("{value} is not a number for {}", input.name))?;
                arms.push((n.to_string(), body(text)));
            } else {
                arms.push((format!("{value:?}"), body(text)));
            }
        }
    }
    let fallback = fallback.ok_or("no * variant")?;
    let mut code = format!("match {scrutinee} {{");
    for (pattern, text) in arms {
        write!(code, "{pattern} => {text},").unwrap();
    }
    write!(code, "_ => {fallback},}}").unwrap();
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(files: [&str; 4]) -> Catalog {
        Catalog {
            origin: "test".to_string(),
            files: files.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn same(file: &str) -> Catalog {
        catalog([file, file, file, file])
    }

    #[test]
    fn rejects_a_key_missing_in_one_locale() {
        let full = r#"{"a": "A", "b": "B"}"#;
        let err = generate("t", &[catalog([full, full, r#"{"a": "A"}"#, full])]).unwrap_err();
        assert_eq!(err, "b is missing in test/fr.json");
    }

    #[test]
    fn rejects_collisions_between_catalogs() {
        let err = generate("t", &[same(r#"{"a": "A"}"#), same(r#"{"a": "B"}"#)]).unwrap_err();
        assert!(err.contains("already defined"), "{err}");
    }

    #[test]
    fn rejects_undeclared_placeholders_in_translations() {
        let en = r#"{"a": "Hi {name}"}"#;
        let de = r#"{"a": "Hallo {nom}"}"#;
        let err = generate("t", &[catalog([en, de, en, en])]).unwrap_err();
        assert!(err.contains("undeclared placeholder {nom}"), "{err}");
    }

    #[test]
    fn rejects_variants_without_a_fallback() {
        let file = r#"{"a": [{"declarations": ["input count", "local p = count: plural"], "selectors": ["p"], "match": {"p=one": "one"}}]}"#;
        let err = generate("t", &[same(file)]).unwrap_err();
        assert!(err.contains("no * variant"), "{err}");
    }

    #[test]
    fn rejects_bad_braces() {
        let err = generate("t", &[same(r#"{"a": "x } y"}"#)]).unwrap_err();
        assert!(err.contains("stray }"), "{err}");
        let err = generate("t", &[same(r#"{"a": "x {y"}"#)]).unwrap_err();
        assert!(err.contains("unclosed {"), "{err}");
    }

    #[test]
    fn types_inputs_and_return_values() {
        let file = r#"{
            "plain": "Cancel",
            "named": "Opening {address}",
            "rich": "Records for <mono>{domain}</mono>",
            "sig_html": "<p>{name}</p>",
            "counted": [{"declarations": ["input count", "input lastChecked", "local p = count: plural"], "selectors": ["p"], "match": {"p=*": "{count} {lastChecked}", "p=one": "one"}}]
        }"#;
        let code = generate("v1", &[same(file)]).unwrap();
        assert!(code.contains("pub fn plain() -> &'static str"));
        assert!(code.contains("pub fn named(address: impl std::fmt::Display) -> String"));
        assert!(code.contains("pub fn rich(domain: impl std::fmt::Display) -> Rich"));
        assert!(code.contains("pub fn sig_html(name: impl std::fmt::Display) -> String"));
        assert!(code.contains(
            "pub fn counted(count: u64, last_checked: impl std::fmt::Display) -> String"
        ));
        assert!(code.contains(
            "match cardinal(Locale::En, count) {Category::One => fill(\"one\", args),_ => fill("
        ));
    }

    #[test]
    fn selects_on_raw_inputs() {
        let en = r#"{"a": [{"declarations": ["input count", "local p = count: plural"], "selectors": ["p"], "match": {"p=one": "1", "p=*": "n"}}],
                      "s": [{"declarations": ["input partstat"], "selectors": ["partstat"], "match": {"partstat=accepted": "yes", "partstat=*": "{partstat}"}}]}"#;
        let pt = r#"{"a": [{"declarations": ["input count", "local p = count: plural"], "selectors": ["count"], "match": {"count=1": "1", "count=*": "n"}}],
                      "s": [{"declarations": ["input partstat"], "selectors": ["partstat"], "match": {"partstat=accepted": "sim", "partstat=*": "{partstat}"}}]}"#;
        let code = generate("v1", &[catalog([en, en, en, pt])]).unwrap();
        assert!(code.contains("Locale::Pt => match count {1 => fill(\"1\", args),_ =>"));
        assert!(code.contains("match partstat.to_string().as_str() {\"accepted\" =>"));
    }

    #[test]
    fn finds_unused_keys_by_path() {
        let sources = "let a = m::desktop_quit(); m::desktop_show_app_long();";
        assert!(used(sources, "desktop_quit"));
        assert!(!used(sources, "desktop_show_app"));
        assert!(used(sources, "desktop_show_app_long"));
    }
}
