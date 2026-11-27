use std::fmt::Write;

pub const OPTICAL: f64 = 0.875;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Regular,
    Solid,
    Brands,
}

impl Style {
    pub fn parse(word: &str) -> Option<Self> {
        match word {
            "regular" => Some(Self::Regular),
            "solid" => Some(Self::Solid),
            "brands" => Some(Self::Brands),
            _ => None,
        }
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Regular => "regular",
            Self::Solid => "solid",
            Self::Brands => "brands",
        }
    }

    pub const fn package(self) -> &'static str {
        match self {
            Self::Regular => "pro-regular-svg-icons",
            Self::Solid => "pro-solid-svg-icons",
            Self::Brands => "free-brands-svg-icons",
        }
    }

    pub const fn variant(self) -> &'static str {
        match self {
            Self::Regular => "Regular",
            Self::Solid => "Solid",
            Self::Brands => "Brands",
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Regular | Self::Brands => "",
            Self::Solid => "Solid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub style: Style,
    pub name: String,
}

impl Entry {
    pub fn pascal(&self) -> String {
        self.name
            .split('-')
            .map(|part| {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect()
    }

    pub fn file(&self) -> String {
        format!("fa{}.js", self.pascal())
    }

    pub fn variant(&self) -> String {
        format!("{}{}", self.pascal(), self.style.suffix())
    }

    pub fn path(&self) -> String {
        format!("fa/{}/{}.svg", self.style.slug(), self.name)
    }
}

pub fn parse_list(text: &str) -> Result<Vec<Entry>, String> {
    let mut entries: Vec<Entry> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let at = index + 1;
        let mut words = line.split_whitespace();
        let (Some(style), Some(name), None) = (words.next(), words.next(), words.next()) else {
            return Err(format!(
                "icons.txt:{at}: expected '<style> <name>', got '{line}'"
            ));
        };
        let Some(style) = Style::parse(style) else {
            return Err(format!(
                "icons.txt:{at}: unknown style '{style}', use regular, solid or brands"
            ));
        };
        let valid = name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
        if !valid {
            return Err(format!(
                "icons.txt:{at}: '{name}' is not a Font Awesome icon name"
            ));
        }
        let entry = Entry {
            style,
            name: name.to_string(),
        };
        if let Some(clash) = entries.iter().find(|e| e.variant() == entry.variant()) {
            return Err(format!(
                "icons.txt:{at}: '{line}' clashes with '{} {}'",
                clash.style.slug(),
                clash.name
            ));
        }
        entries.push(entry);
    }
    Ok(entries)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub width: u32,
    pub height: u32,
    pub paths: Vec<String>,
}

fn var<'a>(source: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("var {name} = ");
    let start = source.find(&key)? + key.len();
    let rest = &source[start..];
    let end = rest.find(";\n").or_else(|| rest.find(';'))?;
    Some(rest[..end].trim())
}

fn quoted(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('\'') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('\'') else {
            break;
        };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

pub fn parse_glyph(source: &str) -> Result<Glyph, String> {
    let number = |name: &str| -> Result<u32, String> {
        var(source, name)
            .ok_or_else(|| format!("no {name}"))?
            .parse()
            .map_err(|_| format!("{name} is not a number"))
    };
    let width = number("width")?;
    let height = number("height")?;
    let data = var(source, "svgPathData").ok_or("no svgPathData")?;
    let paths: Vec<String> = quoted(data).into_iter().filter(|p| !p.is_empty()).collect();
    if paths.is_empty() {
        return Err("svgPathData is empty".into());
    }
    let safe = paths.iter().all(|p| {
        p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" .,-".contains(&b))
    });
    if !safe {
        return Err("svgPathData has unexpected characters".into());
    }
    Ok(Glyph {
        width,
        height,
        paths,
    })
}

pub fn view_box(width: u32, height: u32) -> [f64; 4] {
    let (w, h) = (f64::from(width), f64::from(height));
    let side = w.max(h) / OPTICAL;
    [(w - side) / 2.0, (h - side) / 2.0, side, side]
}

pub fn svg(glyph: &Glyph) -> String {
    let [x, y, w, h] = view_box(glyph.width, glyph.height);
    let mut out = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{x} {y} {w} {h}">"#);
    for d in &glyph.paths {
        let _ = write!(out, r#"<path d="{d}"/>"#);
    }
    out.push_str("</svg>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "'use strict';\nObject.defineProperty(exports, '__esModule', { value: true });\nvar prefix = 'far';\nvar iconName = 'square-dot';\nvar width = 448;\nvar height = 512;\nvar aliases = [];\nvar unicode = 'e000';\nvar svgPathData = 'M0 0L448 0 448 512 0 512z';\n\nexports.definition = {\n  prefix: prefix,\n  iconName: iconName,\n  icon: [\n    width,\n    height,\n    aliases,\n    unicode,\n    svgPathData\n  ]};\n";

    #[test]
    fn reads_a_glyph_file() {
        let glyph = parse_glyph(SAMPLE).unwrap();
        assert_eq!(glyph.width, 448);
        assert_eq!(glyph.height, 512);
        assert_eq!(glyph.paths, vec!["M0 0L448 0 448 512 0 512z".to_string()]);
    }

    #[test]
    fn reads_layered_path_data() {
        let src = SAMPLE.replace(
            "var svgPathData = 'M0 0L448 0 448 512 0 512z';",
            "var svgPathData = ['M0 0h1v1z', 'M2 2h1v1z'];",
        );
        assert_eq!(parse_glyph(&src).unwrap().paths.len(), 2);
    }

    #[test]
    fn refuses_markup_in_path_data() {
        let src = SAMPLE.replace("448 512 0 512z", "\"/><script/>");
        assert!(parse_glyph(&src).is_err());
    }

    #[test]
    fn optical_box_matches_the_web_icon() {
        let [x, y, w, h] = view_box(448, 512);
        let side = 512.0 / OPTICAL;
        assert_eq!([w, h], [side, side]);
        assert_eq!(x, (448.0 - side) / 2.0);
        assert_eq!(y, (512.0 - side) / 2.0);
        let svg = svg(&parse_glyph(SAMPLE).unwrap());
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\""));
        assert!(svg.ends_with("<path d=\"M0 0L448 0 448 512 0 512z\"/></svg>"));
    }

    #[test]
    fn maps_names_to_files_and_variants() {
        let list = parse_list("regular paper-plane-top\nsolid star\n\nbrands apple\nregular h1\n")
            .unwrap();
        assert_eq!(list[0].file(), "faPaperPlaneTop.js");
        assert_eq!(list[0].variant(), "PaperPlaneTop");
        assert_eq!(list[0].path(), "fa/regular/paper-plane-top.svg");
        assert_eq!(list[1].variant(), "StarSolid");
        assert_eq!(list[2].variant(), "Apple");
        assert_eq!(list[3].file(), "faH1.js");
    }

    #[test]
    fn rejects_bad_lines() {
        assert!(
            parse_list("duotone star")
                .unwrap_err()
                .contains("icons.txt:1")
        );
        assert!(parse_list("regular Star").is_err());
        assert!(parse_list("regular star extra").is_err());
        assert!(
            parse_list("regular star\nregular star")
                .unwrap_err()
                .contains("icons.txt:2")
        );
    }
}
