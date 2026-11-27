#[path = "build/fa.rs"]
mod fa;

use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

const MISSING: &str = "Font Awesome Pro packages are missing from node_modules. \
Set FONTAWESOME_NPM_AUTH_TOKEN (or the //npm.fontawesome.com/:_authToken line in ~/.npmrc) \
and run `pnpm install --frozen-lockfile` in desktop-client.";

fn main() {
    if let Err(err) = run() {
        eprintln!("thelemail-ui icons: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let out = PathBuf::from(env::var("OUT_DIR").map_err(|e| e.to_string())?);
    let root = manifest.join("../..");
    let list = manifest.join("icons.txt");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/fa.rs");
    println!("cargo:rerun-if-changed={}", list.display());

    let text = fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    let entries = fa::parse_list(&text)?;

    let modules = root.join("node_modules/@fortawesome");
    for style in [fa::Style::Regular, fa::Style::Solid, fa::Style::Brands] {
        let manifest = modules.join(style.package()).join("package.json");
        println!("cargo:rerun-if-changed={}", manifest.display());
        if !manifest.is_file() {
            return Err(MISSING.into());
        }
    }

    let mut glyphs = Vec::with_capacity(entries.len());
    for entry in &entries {
        let file = modules.join(entry.style.package()).join(entry.file());
        let source = read(&file).map_err(|e| {
            format!(
                "'{} {}': {e}; check the name in icons.txt",
                entry.style.slug(),
                entry.name
            )
        })?;
        let glyph = fa::parse_glyph(&source)
            .map_err(|e| format!("'{} {}': {e}", entry.style.slug(), entry.name))?;
        glyphs.push(fa::svg(&glyph));
    }

    fs::write(out.join("icons.rs"), generate(&entries, &glyphs))
        .map_err(|e| format!("writing icons.rs: {e}"))
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|_| format!("{} not found", path.display()))
}

fn generate(entries: &[fa::Entry], glyphs: &[String]) -> String {
    let mut out = String::new();
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\npub enum IconName {\n");
    for entry in entries {
        let _ = writeln!(out, "    {},", entry.variant());
    }
    out.push_str("}\n\nimpl IconName {\n");
    let _ = writeln!(out, "    pub const ALL: [Self; {}] = [", entries.len());
    for entry in entries {
        let _ = writeln!(out, "        Self::{},", entry.variant());
    }
    out.push_str("    ];\n\n");

    let arms = |out: &mut String, sig: &str, value: &dyn Fn(usize, &fa::Entry) -> String| {
        let _ = writeln!(out, "    {sig} {{\n        match self {{");
        for (i, entry) in entries.iter().enumerate() {
            let _ = writeln!(
                out,
                "            Self::{} => {},",
                entry.variant(),
                value(i, entry)
            );
        }
        out.push_str("        }\n    }\n\n");
    };
    arms(
        &mut out,
        "pub const fn name(self) -> &'static str",
        &|_, e| format!("{:?}", e.name),
    );
    arms(
        &mut out,
        "pub const fn style(self) -> IconStyle",
        &|_, e| format!("IconStyle::{}", e.style.variant()),
    );
    arms(
        &mut out,
        "pub const fn path(self) -> &'static str",
        &|_, e| format!("{:?}", e.path()),
    );
    arms(
        &mut out,
        "pub const fn svg(self) -> &'static [u8]",
        &|i, _| format!("{:?}.as_bytes()", glyphs[i]),
    );

    out.push_str("    pub fn from_path(path: &str) -> Option<Self> {\n        match path {\n");
    for entry in entries {
        let _ = writeln!(
            out,
            "            {:?} => Some(Self::{}),",
            entry.path(),
            entry.variant()
        );
    }
    out.push_str("            _ => None,\n        }\n    }\n}\n");
    out
}
