use std::ops::Range;

use cssparser::{ParseError, Parser, SourcePosition, Token, serialize_string};

use crate::js;

const BLANK: &str = "url(about:blank)";

struct Edit {
    range: Range<usize>,
    replacement: String,
}

#[derive(Default)]
struct Scan {
    edits: Vec<Edit>,
    too_deep: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Context {
    Rules,
    ImageList,
}

fn keepable(url: &str) -> bool {
    let url = js::trim(url);
    js::starts_with_ignore_case(url, "cid:") || js::starts_with_ignore_case(url, "data:")
}

fn is_removed_at_rule(name: &str) -> bool {
    name.eq_ignore_ascii_case("import") || name.eq_ignore_ascii_case("font-face")
}

fn is_image_list(name: &str) -> bool {
    [
        "image-set",
        "-webkit-image-set",
        "image",
        "cross-fade",
        "-webkit-cross-fade",
    ]
    .iter()
    .any(|known| name.eq_ignore_ascii_case(known))
}

fn is_url_function(name: &str) -> bool {
    name.eq_ignore_ascii_case("url") || name.eq_ignore_ascii_case("src")
}

#[derive(Default)]
struct UrlArgument {
    raw: String,
    value: String,
}

fn needs_quoting(raw: &str) -> bool {
    raw.chars()
        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '\'' | '(' | ')' | '\\'))
}

fn url_replacement(argument: &UrlArgument) -> String {
    if !keepable(&argument.value) {
        return BLANK.to_owned();
    }
    let raw = js::trim(&argument.raw);
    let value = js::trim(&argument.value);
    if !needs_quoting(raw) {
        return format!("url({raw})");
    }
    if !needs_quoting(value) {
        return format!("url({value})");
    }
    let mut quoted = String::new();
    let _ = serialize_string(value, &mut quoted);
    format!("url({quoted})")
}

fn unquoted_inner(raw: &str) -> &str {
    let open = raw.find('(').map_or(raw.len(), |i| i + 1);
    let close = if raw.ends_with(')') && raw.len() > open {
        raw.len() - 1
    } else {
        raw.len()
    };
    &raw[open..close]
}

fn quoted_inner(raw: &str) -> &str {
    let quote = raw.chars().next();
    let body = &raw[quote.map_or(0, char::len_utf8)..];
    match quote {
        Some(q) if body.ends_with(q) && !body.ends_with(&format!("\\{q}")) => {
            &body[..body.len() - q.len_utf8()]
        }
        _ => body,
    }
}

fn skip_block<'i>(parser: &mut Parser<'i>) {
    let _ = parser.parse_nested_block(|_| Ok::<(), ParseError<()>>(()));
}

fn consume_at_rule<'i>(parser: &mut Parser<'i>) {
    loop {
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return,
        };
        match token {
            Token::Semicolon => return,
            Token::CurlyBracketBlock => {
                skip_block(parser);
                return;
            }
            Token::Function(_) | Token::ParenthesisBlock | Token::SquareBracketBlock => {
                skip_block(parser)
            }
            _ => {}
        }
    }
}

fn url_function_argument<'i>(parser: &mut Parser<'i>) -> UrlArgument {
    parser
        .parse_nested_block(|block| {
            let mut first: Option<UrlArgument> = None;
            loop {
                let start = block.position();
                let token = match block.next_including_whitespace_and_comments() {
                    Ok(token) => token.clone(),
                    Err(_) => break,
                };
                match token {
                    Token::WhiteSpace(_) | Token::Comment(_) => {}
                    Token::QuotedString(value) if first.is_none() => {
                        let raw = quoted_inner(block.slice_from(start)).to_owned();
                        first = Some(UrlArgument {
                            raw,
                            value: value.to_string(),
                        });
                    }
                    Token::Function(_)
                    | Token::ParenthesisBlock
                    | Token::SquareBracketBlock
                    | Token::CurlyBracketBlock => {
                        skip_block(block);
                        first.get_or_insert_with(UrlArgument::default);
                    }
                    _ => {
                        let raw = block.slice_from(start).to_owned();
                        first.get_or_insert_with(|| UrlArgument {
                            value: raw.clone(),
                            raw,
                        });
                    }
                }
            }
            Ok::<UrlArgument, ParseError<()>>(first.unwrap_or_default())
        })
        .unwrap_or_default()
}

fn walk<'i>(parser: &mut Parser<'i>, context: Context, scan: &mut Scan) {
    loop {
        let start: SourcePosition = parser.position();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(_) => return,
        };
        let range = |parser: &Parser<'i>| start.byte_index()..parser.position().byte_index();
        match token {
            Token::AtKeyword(name) if is_removed_at_rule(&name) => {
                consume_at_rule(parser);
                scan.edits.push(Edit {
                    range: range(parser),
                    replacement: String::new(),
                });
            }
            Token::UnquotedUrl(value) => {
                let raw = unquoted_inner(parser.slice_from(start)).to_owned();
                let replacement = url_replacement(&UrlArgument {
                    raw,
                    value: value.to_string(),
                });
                scan.edits.push(Edit {
                    range: range(parser),
                    replacement,
                });
            }
            Token::BadUrl(_) => {
                scan.edits.push(Edit {
                    range: range(parser),
                    replacement: BLANK.to_owned(),
                });
            }
            Token::Function(name) if is_url_function(&name) => {
                let argument = url_function_argument(parser);
                scan.edits.push(Edit {
                    range: range(parser),
                    replacement: url_replacement(&argument),
                });
            }
            Token::Function(name) if is_image_list(&name) => {
                let entered = parser.parse_nested_block(|block| {
                    walk(block, Context::ImageList, scan);
                    Ok::<(), ParseError<()>>(())
                });
                scan.too_deep |= entered.is_err();
            }
            Token::QuotedString(value) if context == Context::ImageList => {
                if !keepable(&value) {
                    scan.edits.push(Edit {
                        range: range(parser),
                        replacement: BLANK.to_owned(),
                    });
                }
            }
            Token::Function(_)
            | Token::ParenthesisBlock
            | Token::SquareBracketBlock
            | Token::CurlyBracketBlock => {
                let entered = parser.parse_nested_block(|block| {
                    walk(block, Context::Rules, scan);
                    Ok::<(), ParseError<()>>(())
                });
                scan.too_deep |= entered.is_err();
            }
            _ => {}
        }
    }
}

const MAX_PASSES: usize = 8;

fn scrub_once(css: &str) -> Option<String> {
    let mut parser = Parser::new(css);
    let mut scan = Scan::default();
    walk(&mut parser, Context::Rules, &mut scan);
    if scan.too_deep {
        return None;
    }
    let mut edits = scan.edits;
    edits.sort_by_key(|edit| edit.range.start);
    let mut out = String::with_capacity(css.len());
    let mut cursor = 0;
    for edit in edits {
        if edit.range.start < cursor {
            continue;
        }
        out.push_str(&css[cursor..edit.range.start]);
        out.push_str(&edit.replacement);
        cursor = edit.range.end;
    }
    out.push_str(&css[cursor..]);
    Some(out)
}

pub fn scrub_css(css: &str) -> String {
    let Some(mut current) = scrub_once(css) else {
        return String::new();
    };
    for _ in 0..MAX_PASSES {
        let Some(next) = scrub_once(&current) else {
            return String::new();
        };
        if next == current {
            return current;
        }
        current = next;
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_urls_become_blank() {
        assert_eq!(
            scrub_css("a{background:url(https://x.test/a.png)}"),
            "a{background:url(about:blank)}"
        );
        assert_eq!(
            scrub_css("a{background:u\\72l(https://x.test)}"),
            "a{background:url(about:blank)}"
        );
        assert_eq!(
            scrub_css("b{background:url(\"https://x.test/a)\")}"),
            "b{background:url(about:blank)}"
        );
    }

    #[test]
    fn inline_urls_are_kept_without_quotes() {
        assert_eq!(
            scrub_css("a{background:url( 'cid:hero' )}"),
            "a{background:url(cid:hero)}"
        );
    }

    #[test]
    fn imports_and_font_faces_are_removed() {
        assert_eq!(scrub_css("@import 'x.css';p{}"), "p{}");
        assert_eq!(scrub_css("@\\69mport url(x.css);p{}"), "p{}");
        assert_eq!(scrub_css("@font-face{src:url(x)}p{}"), "p{}");
    }

    #[test]
    fn kept_urls_that_need_quotes_stay_valid() {
        let once = scrub_css("a{background:url(\"data:image/svg+xml,<svg xmlns='x'></svg>\")}");
        assert_eq!(
            once,
            "a{background:url(\"data:image/svg+xml,<svg xmlns='x'></svg>\")}"
        );
        assert_eq!(scrub_css(&once), once);
        let unterminated = scrub_css("a{background:url(\"cid:a b");
        assert_eq!(unterminated, "a{background:url(\"cid:a b\")");
        assert_eq!(scrub_css(&unterminated), unterminated);
    }

    #[test]
    fn escaped_schemes_are_judged_decoded() {
        assert_eq!(
            scrub_css("a{background:url(\\63 id:x)}"),
            "a{background:url(cid:x)}"
        );
        assert_eq!(
            scrub_css("a{background:url(\\68ttps://x.test)}"),
            "a{background:url(about:blank)}"
        );
    }

    #[test]
    fn image_set_strings_are_blank() {
        assert_eq!(
            scrub_css(
                "a{background:image-set(\"https://x.test/a.png\" 1x, \"data:image/png;base64,AA\" 2x)}"
            ),
            "a{background:image-set(url(about:blank) 1x, \"data:image/png;base64,AA\" 2x)}"
        );
    }

    #[test]
    fn nesting_beyond_the_tokenizer_limit_drops_the_sheet() {
        let css = format!(
            "{}background:url(https://x.test){}",
            "a{".repeat(100),
            "}".repeat(100)
        );
        assert_eq!(scrub_css(&css), "");
    }

    #[test]
    fn removals_cannot_splice_a_new_url() {
        assert_eq!(
            scrub_css("a{b:url@import x;(https://x.test)}"),
            "a{b:url(about:blank)}"
        );
        assert_eq!(
            scrub_css("a{b:u@font-face{}rl(https://x.test)}"),
            "a{b:url(about:blank)}"
        );
    }

    #[test]
    fn unrelated_css_is_untouched() {
        let css = "td { color : red ; } @media (max-width:600px){.a{width:100%!important}}";
        assert_eq!(scrub_css(css), css);
    }
}
