use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::color::{self, Rgba};
use crate::css::{self, Rule, split_top};

pub const TOKENS_CSS: &str = "packages/core/src/styles/tokens.css";
pub const SHAPES_CSS: &str = "packages/core/src/styles/shapes.css";
pub const LAYOUT_CSS: &str = "apps/mail/src/routes/layout.css";

const ROOT: &str = ":root";
const DARK: &str = "[data-theme='dark']";

pub struct Sources {
    pub tag: String,
    pub tokens: String,
    pub shapes: String,
    pub layout: String,
}

#[derive(Default, Clone)]
struct Vars {
    order: Vec<String>,
    values: BTreeMap<String, String>,
}

impl Vars {
    fn set(&mut self, name: &str, value: &str) {
        if !self.values.contains_key(name) {
            self.order.push(name.to_string());
        }
        self.values.insert(name.to_string(), value.to_string());
    }

    fn resolve(&self, value: &str) -> Result<String, String> {
        self.resolve_depth(value, 0)
    }

    fn resolve_depth(&self, value: &str, depth: usize) -> Result<String, String> {
        if depth > 16 {
            return Err(format!("var() cycle in {value}"));
        }
        let Some(start) = value.find("var(") else {
            return Ok(value.to_string());
        };
        let open = start + 3;
        let mut level = 0;
        let mut close = None;
        for (i, c) in value[open..].char_indices() {
            match c {
                '(' => level += 1,
                ')' => {
                    level -= 1;
                    if level == 0 {
                        close = Some(open + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close.ok_or_else(|| format!("unclosed var() in {value}"))?;
        let inner = &value[open + 1..close];
        let (name, fallback) = match inner.split_once(',') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner.trim(), None),
        };
        let replacement = match (self.values.get(name), fallback) {
            (Some(v), _) => self.resolve_depth(v, depth + 1)?,
            (None, Some(f)) => self.resolve_depth(f, depth + 1)?,
            (None, None) => return Err(format!("undefined {name}")),
        };
        let spliced = format!("{}{}{}", &value[..start], replacement, &value[close + 1..]);
        self.resolve_depth(&spliced, depth + 1)
    }
}

fn root_vars(rules: &[Rule], selector: &str) -> Vars {
    let mut vars = Vars::default();
    for rule in rules.iter().filter(|r| r.selector == selector) {
        for (name, value) in &rule.decls {
            if name.starts_with("--") {
                vars.set(name, value);
            }
        }
    }
    vars
}

fn px(value: &str) -> Result<f64, String> {
    let v = value.trim();
    if v == "0" {
        return Ok(0.0);
    }
    v.strip_suffix("px")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("expected px, got {value}"))
}

fn em(value: &str) -> Result<f64, String> {
    let v = value.trim();
    if v == "0" {
        return Ok(0.0);
    }
    v.strip_suffix("em")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("expected em, got {value}"))
}

fn ms(value: &str) -> Result<u64, String> {
    value
        .trim()
        .strip_suffix("ms")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| format!("expected ms, got {value}"))
}

fn number(value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse()
        .map_err(|_| format!("expected a number, got {value}"))
}

fn percent(value: &str) -> Result<f64, String> {
    value
        .trim()
        .strip_suffix('%')
        .and_then(|n| n.parse::<f64>().ok())
        .map(|n| n / 100.0)
        .ok_or_else(|| format!("expected %, got {value}"))
}

fn bezier(value: &str) -> Result<[f64; 4], String> {
    let args = color::function(value.trim(), "cubic-bezier")
        .ok_or_else(|| format!("expected cubic-bezier, got {value}"))?;
    let parts: Vec<f64> = args.split(',').map(number).collect::<Result<_, _>>()?;
    parts
        .try_into()
        .map_err(|_| format!("cubic-bezier needs four numbers: {value}"))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShadowLayer {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
    pub color: Rgba,
    pub inset: bool,
}

pub fn shadow(value: &str) -> Result<Vec<ShadowLayer>, String> {
    split_top(value, ',')
        .into_iter()
        .map(|layer| {
            let mut inset = false;
            let mut lengths = Vec::new();
            let mut color = None;
            for part in split_top(layer, ' ') {
                if part.is_empty() {
                    continue;
                }
                if part == "inset" {
                    inset = true;
                } else if let Ok(v) = px(part) {
                    lengths.push(v);
                } else {
                    color = Some(color::parse(part)?);
                }
            }
            let color = color.ok_or_else(|| format!("shadow without colour: {layer}"))?;
            let at = |i: usize| lengths.get(i).copied().unwrap_or(0.0);
            if !(2..=4).contains(&lengths.len()) {
                return Err(format!("shadow needs 2 to 4 lengths: {layer}"));
            }
            Ok(ShadowLayer {
                x: at(0),
                y: at(1),
                blur: at(2),
                spread: at(3),
                color,
                inset,
            })
        })
        .collect()
}

fn snake(name: &str) -> String {
    name.trim_start_matches("--").replace('-', "_")
}

fn upper(name: &str) -> String {
    snake(name).to_ascii_uppercase()
}

fn pascal(name: &str) -> String {
    name.trim_start_matches("--")
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn float(v: f64) -> String {
    format!("{:?}", v as f32)
}

fn hex(c: Rgba) -> String {
    format!("0x{:08X}", c.to_u32())
}

fn quote(s: &str) -> String {
    format!("{s:?}")
}

struct FontFace {
    family: String,
    size_adjust: f64,
    min_weight: f64,
    max_weight: f64,
}

fn font_faces(rules: &[Rule]) -> Result<Vec<FontFace>, String> {
    let mut faces: Vec<FontFace> = Vec::new();
    for rule in rules.iter().filter(|r| r.selector == "@font-face") {
        let get = |key: &str| {
            rule.decls
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        };
        let family = get("font-family")
            .ok_or("@font-face without font-family")?
            .trim_matches(['"', '\''])
            .to_string();
        let size_adjust = get("size-adjust").map(percent).transpose()?.unwrap_or(1.0);
        let weights: Vec<f64> = get("font-weight")
            .unwrap_or("400")
            .split_whitespace()
            .map(number)
            .collect::<Result<_, _>>()?;
        let min_weight = *weights.first().ok_or("empty font-weight")?;
        let max_weight = *weights.last().ok_or("empty font-weight")?;
        match faces.iter().find(|f| f.family == family) {
            Some(f) if f.size_adjust != size_adjust => {
                return Err(format!("{family} faces disagree on size-adjust"));
            }
            Some(_) => {}
            None => faces.push(FontFace {
                family,
                size_adjust,
                min_weight,
                max_weight,
            }),
        }
    }
    Ok(faces)
}

fn first_family(value: &str) -> String {
    split_top(value, ',')
        .first()
        .map(|f| f.trim_matches(['"', '\'']).to_string())
        .unwrap_or_default()
}

pub fn generate(src: &Sources) -> Result<String, String> {
    let tokens = css::parse(&src.tokens);
    let shapes = css::parse(&src.shapes);
    let layout = css::parse(&src.layout);

    let color_vars = root_vars(&tokens, ROOT);
    let dark_overrides = root_vars(&tokens, DARK);
    let shape_vars = root_vars(&shapes, ROOT);
    let layout_vars = root_vars(&layout, ROOT);

    let mut light = Vars::default();
    for vars in [&color_vars, &shape_vars, &layout_vars] {
        for name in &vars.order {
            light.set(name, &vars.values[name]);
        }
    }
    let mut dark = light.clone();
    for name in &dark_overrides.order {
        if !color_vars.values.contains_key(name) {
            return Err(format!("{name} is set for dark only"));
        }
        dark.set(name, &dark_overrides.values[name]);
    }

    let mut out = String::new();
    writeln!(out, "use std::time::Duration;").unwrap();
    writeln!(out, "pub const SOURCE_TAG: &str = {};", quote(&src.tag)).unwrap();

    emit_colors(&mut out, &color_vars, &light, &dark)?;
    emit_shadows(&mut out, &color_vars, &light, &dark)?;
    emit_shapes(&mut out, &shape_vars, &light)?;
    let families = emit_type(&mut out, &layout_vars, &light, &font_faces(&layout)?)?;
    emit_text_styles(&mut out, &layout, &light, &color_vars, &families)?;
    Ok(out)
}

fn color_names(color_vars: &Vars) -> Vec<&String> {
    color_vars
        .order
        .iter()
        .filter(|n| !n.starts_with("--shadow-"))
        .collect()
}

fn emit_colors(
    out: &mut String,
    color_vars: &Vars,
    light: &Vars,
    dark: &Vars,
) -> Result<(), String> {
    let names = color_names(color_vars);
    let resolve = |vars: &Vars, name: &str| -> Result<Rgba, String> {
        color::parse(&vars.resolve(&vars.values[name])?).map_err(|e| format!("{name}: {e}"))
    };

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "pub enum ColorToken {{").unwrap();
    for name in &names {
        writeln!(out, "{},", pascal(name)).unwrap();
    }
    writeln!(out, "}}").unwrap();

    writeln!(out, "impl ColorToken {{").unwrap();
    writeln!(out, "pub const ALL: [Self; {}] = [", names.len()).unwrap();
    for name in &names {
        writeln!(out, "Self::{},", pascal(name)).unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(
        out,
        "pub const fn name(self) -> &'static str {{ match self {{"
    )
    .unwrap();
    for name in &names {
        writeln!(
            out,
            "Self::{} => {},",
            pascal(name),
            quote(name.trim_start_matches("--"))
        )
        .unwrap();
    }
    writeln!(out, "}} }} }}").unwrap();

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq, Eq)]").unwrap();
    writeln!(out, "pub struct Palette {{").unwrap();
    for name in &names {
        writeln!(out, "pub {}: u32,", snake(name)).unwrap();
    }
    writeln!(out, "}}").unwrap();

    writeln!(out, "impl Palette {{").unwrap();
    writeln!(
        out,
        "pub const fn get(&self, token: ColorToken) -> u32 {{ match token {{"
    )
    .unwrap();
    for name in &names {
        writeln!(out, "ColorToken::{} => self.{},", pascal(name), snake(name)).unwrap();
    }
    writeln!(out, "}} }} }}").unwrap();

    for (constant, vars) in [("LIGHT", light), ("DARK", dark)] {
        writeln!(out, "pub const {constant}: Palette = Palette {{").unwrap();
        for name in &names {
            writeln!(out, "{}: {},", snake(name), hex(resolve(vars, name)?)).unwrap();
        }
        writeln!(out, "}};").unwrap();
    }
    Ok(())
}

fn emit_shadows(
    out: &mut String,
    color_vars: &Vars,
    light: &Vars,
    dark: &Vars,
) -> Result<(), String> {
    let names: Vec<&String> = color_vars
        .order
        .iter()
        .filter(|n| n.starts_with("--shadow-"))
        .collect();
    let short = |n: &str| n.trim_start_matches("--shadow-").to_string();

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]").unwrap();
    writeln!(out, "pub struct ShadowLayer {{ pub x: f32, pub y: f32, pub blur: f32, pub spread: f32, pub color: u32, pub inset: bool }}").unwrap();

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "pub enum ShadowToken {{").unwrap();
    for name in &names {
        writeln!(out, "{},", pascal(&short(name))).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out, "impl ShadowToken {{").unwrap();
    writeln!(out, "pub const ALL: [Self; {}] = [", names.len()).unwrap();
    for name in &names {
        writeln!(out, "Self::{},", pascal(&short(name))).unwrap();
    }
    writeln!(out, "];").unwrap();
    writeln!(
        out,
        "pub const fn name(self) -> &'static str {{ match self {{"
    )
    .unwrap();
    for name in &names {
        writeln!(
            out,
            "Self::{} => {},",
            pascal(&short(name)),
            quote(&short(name))
        )
        .unwrap();
    }
    writeln!(out, "}} }} }}").unwrap();

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]").unwrap();
    writeln!(out, "pub struct Shadows {{").unwrap();
    for name in &names {
        writeln!(out, "pub {}: &'static [ShadowLayer],", snake(&short(name))).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out, "impl Shadows {{").unwrap();
    writeln!(
        out,
        "pub const fn get(&self, token: ShadowToken) -> &'static [ShadowLayer] {{ match token {{"
    )
    .unwrap();
    for name in &names {
        writeln!(
            out,
            "ShadowToken::{} => self.{},",
            pascal(&short(name)),
            snake(&short(name))
        )
        .unwrap();
    }
    writeln!(out, "}} }} }}").unwrap();

    for (constant, vars) in [("LIGHT_SHADOWS", light), ("DARK_SHADOWS", dark)] {
        writeln!(out, "pub const {constant}: Shadows = Shadows {{").unwrap();
        for name in &names {
            let layers =
                shadow(&vars.resolve(&vars.values[*name])?).map_err(|e| format!("{name}: {e}"))?;
            write!(out, "{}: &[", snake(&short(name))).unwrap();
            for l in layers {
                write!(
                    out,
                    "ShadowLayer {{ x: {}, y: {}, blur: {}, spread: {}, color: {}, inset: {} }},",
                    float(l.x),
                    float(l.y),
                    float(l.blur),
                    float(l.spread),
                    hex(l.color),
                    l.inset
                )
                .unwrap();
            }
            writeln!(out, "],").unwrap();
        }
        writeln!(out, "}};").unwrap();
    }
    Ok(())
}

fn emit_shapes(out: &mut String, shape_vars: &Vars, vars: &Vars) -> Result<(), String> {
    let mut radii = Vec::new();
    let mut tied = BTreeMap::new();
    let mut controls = Vec::new();
    let mut layers = Vec::new();
    for name in &shape_vars.order {
        let value = vars.resolve(&shape_vars.values[name])?;
        let short = name.trim_start_matches("--");
        if name == "--radius" {
            continue;
        }
        if let Some(h) = short.strip_prefix("radius-h") {
            tied.insert(h.to_string(), px(&value)?);
        } else if let Some(r) = short.strip_prefix("radius-") {
            if value.ends_with('%') {
                writeln!(
                    out,
                    "pub const RADIUS_{}_FRACTION: f32 = {};",
                    upper(r),
                    float(percent(&value)?)
                )
                .unwrap();
            } else {
                radii.push((r.to_string(), px(&value)?));
            }
        } else if let Some(c) = short.strip_prefix("control-h-") {
            controls.push((c.to_string(), px(&value)?));
        } else if let Some(z) = short.strip_prefix("z-") {
            layers.push((z.to_string(), number(&value)?));
        } else {
            return Err(format!("unknown shape token {name}"));
        }
    }

    for (name, v) in &radii {
        writeln!(
            out,
            "pub const RADIUS_{}: f32 = {};",
            upper(name),
            float(*v)
        )
        .unwrap();
    }
    writeln!(out, "pub const RADII: [(&str, f32); {}] = [", radii.len()).unwrap();
    for (name, _) in &radii {
        writeln!(out, "({}, RADIUS_{}),", quote(name), upper(name)).unwrap();
    }
    writeln!(out, "];").unwrap();

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]").unwrap();
    writeln!(
        out,
        "pub struct ControlHeight {{ pub name: &'static str, pub height: f32, pub radius: f32 }}"
    )
    .unwrap();
    for (name, h) in &controls {
        let key = format!("{h}");
        let radius = tied
            .remove(&key)
            .ok_or_else(|| format!("control height {h} has no tied radius"))?;
        writeln!(
            out,
            "pub const CONTROL_{}: ControlHeight = ControlHeight {{ name: {}, height: {}, radius: {} }};",
            upper(name),
            quote(name),
            float(*h),
            float(radius)
        )
        .unwrap();
    }
    if let Some(h) = tied.keys().next() {
        return Err(format!("radius-h{h} has no control height"));
    }
    writeln!(
        out,
        "pub const CONTROL_HEIGHTS: [ControlHeight; {}] = [",
        controls.len()
    )
    .unwrap();
    for (name, _) in &controls {
        writeln!(out, "CONTROL_{},", upper(name)).unwrap();
    }
    writeln!(out, "];").unwrap();

    for (name, v) in &layers {
        writeln!(out, "pub const Z_{}: u32 = {};", upper(name), *v as u32).unwrap();
    }
    writeln!(
        out,
        "pub const Z_LAYERS: [(&str, u32); {}] = [",
        layers.len()
    )
    .unwrap();
    for (name, _) in &layers {
        writeln!(out, "({}, Z_{}),", quote(name), upper(name)).unwrap();
    }
    writeln!(out, "];").unwrap();
    Ok(())
}

fn emit_type(
    out: &mut String,
    layout_vars: &Vars,
    vars: &Vars,
    faces: &[FontFace],
) -> Result<BTreeMap<String, String>, String> {
    let mut families = BTreeMap::new();
    let mut weights = Vec::new();
    let mut sizes = Vec::new();
    let mut line_heights = BTreeMap::new();
    let mut tracking = Vec::new();
    let mut spacing = Vec::new();
    let mut curves = Vec::new();
    let mut durations = Vec::new();

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]").unwrap();
    writeln!(out, "pub struct Family {{ pub name: &'static str, pub size_adjust: f32, pub min_weight: f32, pub max_weight: f32 }}").unwrap();

    for name in &layout_vars.order {
        let short = name.trim_start_matches("--");
        let value = vars.resolve(&layout_vars.values[name])?;
        if let Some(f) = short.strip_prefix("font-") {
            let family = first_family(&value);
            let face = faces
                .iter()
                .find(|face| face.family == family)
                .ok_or_else(|| format!("{name}: no @font-face for {family}"))?;
            let constant = format!("FONT_{}", upper(f));
            writeln!(
                out,
                "pub const {constant}: Family = Family {{ name: {}, size_adjust: {}, min_weight: {}, max_weight: {} }};",
                quote(&family),
                float(face.size_adjust),
                float(face.min_weight),
                float(face.max_weight)
            )
            .unwrap();
            families.insert(name.clone(), constant);
        } else if let Some(w) = short.strip_prefix("w-") {
            weights.push((w.to_string(), number(&value)?));
        } else if let Some(l) = short.strip_prefix("lh-") {
            line_heights.insert(l.to_string(), px(&value)?);
        } else if let Some(t) = short.strip_prefix("text-") {
            sizes.push((format!("text-{t}"), t.to_string(), px(&value)?));
        } else if let Some(d) = short.strip_prefix("display-") {
            sizes.push((format!("display-{d}"), format!("display-{d}"), px(&value)?));
        } else if let Some(t) = short.strip_prefix("track-") {
            tracking.push((t.to_string(), em(&value)?));
        } else if let Some(s) = short.strip_prefix("space-") {
            spacing.push((s.to_string(), px(&value)?));
        } else if short.starts_with("ease") {
            curves.push((short.to_string(), bezier(&value)?));
        } else if short.starts_with("dur") {
            durations.push((short.to_string(), ms(&value)?));
        } else if short == "hairline" || short == "rule" {
            continue;
        } else {
            return Err(format!("unknown layout token {name}"));
        }
    }

    for (name, v) in &weights {
        writeln!(
            out,
            "pub const WEIGHT_{}: f32 = {};",
            upper(name),
            float(*v)
        )
        .unwrap();
    }

    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]").unwrap();
    writeln!(
        out,
        "pub struct TypeSize {{ pub name: &'static str, pub size: f32, pub line_height: f32 }}"
    )
    .unwrap();
    for (name, lh_key, size) in &sizes {
        let lh = line_heights
            .remove(lh_key)
            .ok_or_else(|| format!("{name} has no line height"))?;
        writeln!(
            out,
            "pub const {}: TypeSize = TypeSize {{ name: {}, size: {}, line_height: {} }};",
            upper(name),
            quote(name),
            float(*size),
            float(lh)
        )
        .unwrap();
    }
    if let Some(l) = line_heights.keys().next() {
        return Err(format!("lh-{l} has no size"));
    }
    writeln!(out, "pub const TYPE_SCALE: [TypeSize; {}] = [", sizes.len()).unwrap();
    for (name, _, _) in &sizes {
        writeln!(out, "{},", upper(name)).unwrap();
    }
    writeln!(out, "];").unwrap();

    for (name, v) in &tracking {
        writeln!(out, "pub const TRACK_{}: f32 = {};", upper(name), float(*v)).unwrap();
    }
    writeln!(
        out,
        "pub const TRACKING: [(&str, f32); {}] = [",
        tracking.len()
    )
    .unwrap();
    for (name, _) in &tracking {
        writeln!(out, "({}, TRACK_{}),", quote(name), upper(name)).unwrap();
    }
    writeln!(out, "];").unwrap();

    for (name, v) in &spacing {
        writeln!(out, "pub const SPACE_{}: f32 = {};", upper(name), float(*v)).unwrap();
    }
    writeln!(
        out,
        "pub const SPACING: [(&str, f32); {}] = [",
        spacing.len()
    )
    .unwrap();
    for (name, _) in &spacing {
        writeln!(out, "({}, SPACE_{}),", quote(name), upper(name)).unwrap();
    }
    writeln!(out, "];").unwrap();

    for (name, [a, b, c, d]) in &curves {
        writeln!(
            out,
            "pub const {}: [f32; 4] = [{}, {}, {}, {}];",
            upper(name),
            float(*a),
            float(*b),
            float(*c),
            float(*d)
        )
        .unwrap();
    }
    for (name, v) in &durations {
        writeln!(
            out,
            "pub const {}: Duration = Duration::from_millis({v});",
            upper(name)
        )
        .unwrap();
    }
    Ok(families)
}

fn emit_text_styles(
    out: &mut String,
    layout: &[Rule],
    vars: &Vars,
    color_vars: &Vars,
    families: &BTreeMap<String, String>,
) -> Result<(), String> {
    writeln!(out, "#[derive(Clone, Copy, Debug, PartialEq)]").unwrap();
    writeln!(out, "pub struct TextStyleToken {{ pub name: &'static str, pub family: Family, pub weight: f32, pub size: f32, pub line_height: f32, pub tracking: f32, pub color: ColorToken, pub uppercase: bool }}").unwrap();

    let var_name = |value: &str| -> Option<String> {
        color::function(value.trim(), "var").map(|n| n.trim().to_string())
    };

    let mut names = Vec::new();
    for rule in layout.iter().filter(|r| r.selector.starts_with(".t-")) {
        let name = rule.selector.trim_start_matches('.');
        if name.contains([' ', ':', ',']) {
            return Err(format!("unexpected text style selector {}", rule.selector));
        }
        let mut family = None;
        let mut weight = None;
        let mut size = None;
        let mut line_height = None;
        let mut tracking = 0.0;
        let mut color = None;
        let mut uppercase = false;
        for (prop, value) in &rule.decls {
            match prop.as_str() {
                "font-family" => {
                    let var = var_name(value)
                        .ok_or_else(|| format!("{name}: font-family must be a token"))?;
                    family = Some(
                        families
                            .get(&var)
                            .ok_or_else(|| format!("{name}: unknown family {var}"))?
                            .clone(),
                    );
                }
                "font-weight" => weight = Some(number(&vars.resolve(value)?)?),
                "font-size" => size = Some(px(&vars.resolve(value)?)?),
                "line-height" => line_height = Some(px(&vars.resolve(value)?)?),
                "letter-spacing" => tracking = em(&vars.resolve(value)?)?,
                "color" => {
                    let var =
                        var_name(value).ok_or_else(|| format!("{name}: color must be a token"))?;
                    if !color_vars.values.contains_key(&var) {
                        return Err(format!("{name}: unknown colour {var}"));
                    }
                    color = Some(pascal(&var));
                }
                "text-transform" if value == "uppercase" => uppercase = true,
                "font-feature-settings" if value == "normal" => {}
                _ => return Err(format!("{name}: unsupported {prop}: {value}")),
            }
        }
        let missing = |what: &str| format!("{name}: no {what}");
        let constant = format!("T_{}", upper(name.trim_start_matches("t-")));
        writeln!(
            out,
            "pub const {constant}: TextStyleToken = TextStyleToken {{ name: {}, family: {}, weight: {}, size: {}, line_height: {}, tracking: {}, color: ColorToken::{}, uppercase: {} }};",
            quote(name),
            family.ok_or_else(|| missing("font-family"))?,
            float(weight.ok_or_else(|| missing("font-weight"))?),
            float(size.ok_or_else(|| missing("font-size"))?),
            float(line_height.ok_or_else(|| missing("line-height"))?),
            float(tracking),
            color.ok_or_else(|| missing("color"))?,
            uppercase
        )
        .unwrap();
        names.push(constant);
    }
    writeln!(
        out,
        "pub const TEXT_STYLES: [TextStyleToken; {}] = [",
        names.len()
    )
    .unwrap();
    for name in &names {
        writeln!(out, "{name},").unwrap();
    }
    writeln!(out, "];").unwrap();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources(tokens: &str, shapes: &str, layout: &str) -> Sources {
        Sources {
            tag: "v0".into(),
            tokens: tokens.into(),
            shapes: shapes.into(),
            layout: layout.into(),
        }
    }

    const LAYOUT: &str = "@font-face{font-family:\"Sans\";font-weight:100 900;size-adjust:106%;src:url(a)}\n:root { --font-sans: \"Sans\", system-ui; --w-regular: 400; --text-xs: 12px; --lh-xs: 16px; --track-caps: 0.1em; --space-1: 4px; --ease: cubic-bezier(0.2, 0.6, 0.2, 1); --dur: 200ms; }\n@layer components { .t-eyebrow { font-family: var(--font-sans); font-weight: var(--w-regular); font-size: var(--text-xs); line-height: var(--lh-xs); letter-spacing: var(--track-caps); text-transform: uppercase; color: var(--brass-600); } }";
    const SHAPES: &str = ":root { --radius-xs: 4px; --radius-check: var(--radius-xs); --radius-avatar: 22%; --radius: var(--radius-xs); --control-h-xs: 24px; --radius-h24: 5px; --z-dialog: 1100; }";

    #[test]
    fn dark_resolves_references_against_dark_values() {
        let out = generate(&sources(
            ":root { --brass-100: #F9E7CB; --brass-700: #5D4104; --brass-600: #765407; --on: var(--brass-100); }\n[data-theme='dark'] { --brass-100: #392B14; --on: var(--brass-700); }",
            SHAPES,
            LAYOUT,
        ))
        .unwrap();
        assert!(out.contains("pub const LIGHT: Palette = Palette {\nbrass_100: 0xF9E7CBFF,\nbrass_700: 0x5D4104FF,\nbrass_600: 0x765407FF,\non: 0xF9E7CBFF,"));
        assert!(out.contains("pub const DARK: Palette = Palette {\nbrass_100: 0x392B14FF,\nbrass_700: 0x5D4104FF,\nbrass_600: 0x765407FF,\non: 0x5D4104FF,"));
    }

    #[test]
    fn emits_shapes_type_and_text_styles() {
        let out = generate(&sources(
            ":root { --brass-600: #765407; --shadow-xs: 0 1px 1px rgba(16, 18, 20, 0.04); }",
            SHAPES,
            LAYOUT,
        ))
        .unwrap();
        assert!(out.contains("pub const RADIUS_CHECK: f32 = 4.0;"));
        assert!(out.contains("pub const RADIUS_AVATAR_FRACTION: f32 = 0.22;"));
        assert!(out.contains("pub const CONTROL_XS: ControlHeight = ControlHeight { name: \"xs\", height: 24.0, radius: 5.0 };"));
        assert!(out.contains("pub const Z_DIALOG: u32 = 1100;"));
        assert!(out.contains("pub const FONT_SANS: Family = Family { name: \"Sans\", size_adjust: 1.06, min_weight: 100.0, max_weight: 900.0 };"));
        assert!(out.contains("pub const TEXT_XS: TypeSize = TypeSize { name: \"text-xs\", size: 12.0, line_height: 16.0 };"));
        assert!(out.contains("pub const EASE: [f32; 4] = [0.2, 0.6, 0.2, 1.0];"));
        assert!(out.contains("pub const DUR: Duration = Duration::from_millis(200);"));
        assert!(out.contains("pub const T_EYEBROW: TextStyleToken = TextStyleToken { name: \"t-eyebrow\", family: FONT_SANS, weight: 400.0, size: 12.0, line_height: 16.0, tracking: 0.1, color: ColorToken::Brass600, uppercase: true };"));
        assert!(out.contains("xs: &[ShadowLayer { x: 0.0, y: 1.0, blur: 1.0, spread: 0.0, color: 0x1012140A, inset: false },],"));
    }

    #[test]
    fn rejects_unknown_tokens() {
        let err = generate(&sources(
            ":root { --brass-600: #765407; }",
            SHAPES,
            ":root { --mystery: 3px; }",
        ))
        .unwrap_err();
        assert_eq!(err, "unknown layout token --mystery");
        let err = generate(&sources(
            ":root { --a: #fff; }\n[data-theme='dark'] { --b: #000; }",
            SHAPES,
            LAYOUT,
        ))
        .unwrap_err();
        assert_eq!(err, "--b is set for dark only");
    }

    #[test]
    fn parses_shadow_lists() {
        let layers = shadow("inset 0 1px 2px rgba(0, 0, 0, 0.3), 0 0 0 0.5px #000").unwrap();
        assert_eq!(layers.len(), 2);
        assert!(layers[0].inset);
        assert_eq!((layers[0].y, layers[0].blur), (1.0, 2.0));
        assert_eq!(layers[1].spread, 0.5);
        assert!(!layers[1].inset);
    }
}
