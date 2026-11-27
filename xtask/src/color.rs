use crate::css::split_top;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    pub fn to_u32(self) -> u32 {
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
        (byte(self.r) << 24) | (byte(self.g) << 16) | (byte(self.b) << 8) | byte(self.a)
    }
}

pub fn parse(value: &str) -> Result<Rgba, String> {
    let value = value.trim();
    if value == "transparent" {
        return Ok(Rgba::TRANSPARENT);
    }
    if let Some(hex) = value.strip_prefix('#') {
        return hex_color(hex).ok_or_else(|| format!("bad hex colour {value}"));
    }
    if let Some(args) = function(value, "rgba").or_else(|| function(value, "rgb")) {
        return rgb_function(args).ok_or_else(|| format!("bad rgb colour {value}"));
    }
    if let Some(args) = function(value, "color-mix") {
        return color_mix(args);
    }
    Err(format!("unsupported colour {value}"))
}

pub fn function<'a>(value: &'a str, name: &str) -> Option<&'a str> {
    value
        .strip_prefix(name)?
        .trim_start()
        .strip_prefix('(')?
        .strip_suffix(')')
}

fn hex_color(hex: &str) -> Option<Rgba> {
    let digits: Vec<u8> = match hex.len() {
        3 | 4 => hex
            .chars()
            .map(|c| u8::from_str_radix(&c.to_string(), 16).map(|v| v * 17))
            .collect::<Result<_, _>>()
            .ok()?,
        6 | 8 => (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
            .collect::<Option<_>>()?,
        _ => return None,
    };
    let channel = |i: usize| digits.get(i).map(|v| f64::from(*v) / 255.0);
    Some(Rgba {
        r: channel(0)?,
        g: channel(1)?,
        b: channel(2)?,
        a: channel(3).unwrap_or(1.0),
    })
}

fn rgb_function(args: &str) -> Option<Rgba> {
    let parts: Vec<f64> = args
        .split([',', '/', ' '])
        .filter(|p| !p.trim().is_empty())
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    let (r, g, b) = (parts.first()?, parts.get(1)?, parts.get(2)?);
    Some(Rgba {
        r: r / 255.0,
        g: g / 255.0,
        b: b / 255.0,
        a: parts.get(3).copied().unwrap_or(1.0),
    })
}

fn color_mix(args: &str) -> Result<Rgba, String> {
    let parts = split_top(args, ',');
    let [space, first, second] = parts.as_slice() else {
        return Err(format!("color-mix needs three arguments: {args}"));
    };
    if space.trim() != "in oklab" {
        return Err(format!("unsupported color-mix space {space}"));
    }
    let (a, pa) = mix_stop(first)?;
    let (b, pb) = mix_stop(second)?;
    let (pa, pb) = match (pa, pb) {
        (Some(pa), Some(pb)) => (pa, pb),
        (Some(pa), None) => (pa, 1.0 - pa),
        (None, Some(pb)) => (1.0 - pb, pb),
        (None, None) => (0.5, 0.5),
    };
    let total = pa + pb;
    if total <= 0.0 {
        return Err(format!("color-mix weights sum to zero: {args}"));
    }
    let mixed = mix_oklab(a, b, pb / total);
    Ok(Rgba {
        a: mixed.a * total.min(1.0),
        ..mixed
    })
}

fn mix_stop(stop: &str) -> Result<(Rgba, Option<f64>), String> {
    let stop = stop.trim();
    match stop.rsplit_once(' ') {
        Some((color, pct)) if pct.ends_with('%') => {
            let pct: f64 = pct
                .trim_end_matches('%')
                .parse()
                .map_err(|_| format!("bad percentage in {stop}"))?;
            Ok((parse(color)?, Some(pct / 100.0)))
        }
        _ => Ok((parse(stop)?, None)),
    }
}

pub fn mix_oklab(a: Rgba, b: Rgba, t: f64) -> Rgba {
    let la = to_oklab(a);
    let lb = to_oklab(b);
    let alpha = a.a * (1.0 - t) + b.a * t;
    if alpha <= 0.0 {
        return Rgba::TRANSPARENT;
    }
    let channel = |x: f64, y: f64| (x * a.a * (1.0 - t) + y * b.a * t) / alpha;
    from_oklab(
        [
            channel(la[0], lb[0]),
            channel(la[1], lb[1]),
            channel(la[2], lb[2]),
        ],
        alpha,
    )
}

fn to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

pub fn to_oklab(c: Rgba) -> [f64; 3] {
    let (r, g, b) = (to_linear(c.r), to_linear(c.g), to_linear(c.b));
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

pub fn from_oklab([l, a, b]: [f64; 3], alpha: f64) -> Rgba {
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s_ = (l - 0.0894841775 * a - 1.2914855480 * b).powi(3);
    Rgba {
        r: to_gamma(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
        g: to_gamma(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
        b: to_gamma(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.7076147010 * s_),
        a: alpha,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_and_rgba() {
        assert_eq!(parse("#FBFAF8").unwrap().to_u32(), 0xFBFAF8FF);
        assert_eq!(parse("#B0DEC48C").unwrap().to_u32(), 0xB0DEC48C);
        assert_eq!(
            parse("rgba(11, 13, 16, 0.54)").unwrap().to_u32(),
            0x0B0D108A
        );
        assert_eq!(parse("transparent").unwrap().to_u32(), 0);
    }

    #[test]
    fn oklab_round_trips() {
        for hex in ["#005C3B", "#FEE2DE", "#131619", "#FFFFFF", "#000000"] {
            let c = parse(hex).unwrap();
            assert_eq!(from_oklab(to_oklab(c), 1.0).to_u32(), c.to_u32());
        }
        let white = to_oklab(parse("#FFFFFF").unwrap());
        assert!((white[0] - 1.0).abs() < 1e-6);
        assert!(white[1].abs() < 1e-6 && white[2].abs() < 1e-6);
    }

    #[test]
    fn mixing_with_transparent_keeps_the_colour() {
        let c = parse("color-mix(in oklab, #21201F 7%, transparent)").unwrap();
        assert_eq!(c.to_u32(), 0x21201F12);
    }

    #[test]
    fn mixes_two_opaque_colours() {
        let c = parse("color-mix(in oklab, #000000 50%, #FFFFFF)").unwrap();
        let l = to_oklab(c)[0];
        assert!((l - 0.5).abs() < 0.003, "{l}");
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn matches_chrome_for_the_token_mixes() {
        let cases = [
            ("#D6F1E2 82%, #095436", 0xB2D3C0FF),
            ("#FEE2DE 85%, #7A2520", 0xEBC5BFFF),
            ("#B74C44 88%, #131312", 0xA1453EFF),
            ("#1E3328 82%, #90D2AE", 0x304D3EFF),
            ("#432420 85%, #FEA69C", 0x5C3530FF),
            ("#E27368 88%, #EFF1F4", 0xE68379FF),
        ];
        for (args, chrome) in cases {
            let mixed = parse(&format!("color-mix(in oklab, {args})")).unwrap();
            assert_eq!(mixed.to_u32(), chrome, "{args}");
        }
    }

    #[test]
    fn rejects_other_spaces() {
        assert!(parse("color-mix(in srgb, red 50%, blue)").is_err());
    }
}
