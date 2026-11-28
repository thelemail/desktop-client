use std::ops::Range;

use cssparser::{ParseError, Parser, SourcePosition, Token};

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

fn url_replacement(raw_inner: &str) -> String {
    if keepable(raw_inner) {
        format!("url({})", js::trim(raw_inner))
    } else {
        BLANK.to_owned()
    }
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

fn url_function_inner<'i>(parser: &mut Parser<'i>) -> String {
    parser
        .parse_nested_block(|block| {
            let mut first: Option<String> = None;
            loop {
                let start = block.position();
                let token = match block.next_including_whitespace_and_comments() {
                    Ok(token) => token.clone(),
                    Err(_) => break,
                };
                match token {
                    Token::WhiteSpace(_) | Token::Comment(_) => {}
                    Token::QuotedString(_) if first.is_none() => {
                        first = Some(quoted_inner(block.slice_from(start)).to_owned());
                    }
                    Token::Function(_)
                    | Token::ParenthesisBlock
                    | Token::SquareBracketBlock
                    | Token::CurlyBracketBlock => {
                        skip_block(block);
                        first.get_or_insert_with(String::new);
                    }
                    _ => {
                        first.get_or_insert_with(|| block.slice_from(start).to_owned());
                    }
                }
            }
            Ok::<String, ParseError<()>>(first.unwrap_or_default())
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
            Token::UnquotedUrl(_) => {
                let replacement = url_replacement(unquoted_inner(parser.slice_from(start)));
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
                let inner = url_function_inner(parser);
                scan.edits.push(Edit {
                    range: range(parser),
                    replacement: url_replacement(&inner),
                });
            }
            Token::Function(name) if is_image_list(&name) => {
                let entered = parser.parse_nested_block(|block| {
                    walk(block, Context::ImageList, scan);
                    Ok::<(), ParseError<()>>(())
                });
                scan.too_deep |= entered.is_err();
            }
            Token::QuotedString(_) if context == Context::ImageList => {
                if !keepable(quoted_inner(parser.slice_from(start))) {
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

pub fn scrub_css(css: &str) -> String {
    let mut parser = Parser::new(css);
    let mut scan = Scan::default();
    walk(&mut parser, Context::Rules, &mut scan);
    if scan.too_deep {
        return String::new();
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
    out
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
    fn unrelated_css_is_untouched() {
        let css = "td { color : red ; } @media (max-width:600px){.a{width:100%!important}}";
        assert_eq!(scrub_css(css), css);
    }
}
