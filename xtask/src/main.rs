mod color;
mod css;
mod i18n;
mod tokens;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const TOKENS_RS: &str = "crates/thelemail-ui/src/tokens.rs";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["tokens"] => run_tokens(false),
        ["tokens", "--check"] => run_tokens(true),
        ["i18n"] => run_i18n(false),
        ["i18n", "--check"] => run_i18n(true),
        _ => Err("usage: cargo xtask <tokens|i18n> [--check]".to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err}");
            ExitCode::FAILURE
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

fn pinned_tag(root: &Path) -> Result<String, String> {
    std::fs::read_to_string(root.join("web-client.version"))
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("reading web-client.version: {e}"))
}

fn git_show(root: &Path, tag: &str, path: &str) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root.join("web-client"))
        .arg("show")
        .arg(format!("{tag}:{path}"))
        .output()
        .map_err(|e| format!("running git: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git show {tag}:{path} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("{path} is not UTF-8: {e}"))
}

fn rustfmt(source: &str) -> Result<String, String> {
    let mut child = Command::new("rustfmt")
        .args(["--edition", "2024", "--emit", "stdout"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("running rustfmt: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("rustfmt has no stdin")?
        .write_all(source.as_bytes())
        .map_err(|e| format!("writing to rustfmt: {e}"))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("waiting for rustfmt: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustfmt failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("rustfmt output is not UTF-8: {e}"))
}

fn run_tokens(check: bool) -> Result<(), String> {
    let root = root();
    let tag = pinned_tag(&root)?;
    let sources = tokens::Sources {
        tokens: git_show(&root, &tag, tokens::TOKENS_CSS)?,
        shapes: git_show(&root, &tag, tokens::SHAPES_CSS)?,
        layout: git_show(&root, &tag, tokens::LAYOUT_CSS)?,
        tag,
    };
    let generated = rustfmt(&tokens::generate(&sources)?)?;
    let target = root.join(TOKENS_RS);
    if check {
        let current = std::fs::read_to_string(&target).unwrap_or_default();
        if current != generated {
            return Err(format!(
                "{TOKENS_RS} is out of date with web-client {}; run cargo xtask tokens",
                sources.tag
            ));
        }
        return Ok(());
    }
    std::fs::write(&target, generated).map_err(|e| format!("writing {TOKENS_RS}: {e}"))
}

fn desktop_catalog(root: &Path) -> Result<i18n::Catalog, String> {
    let files = i18n::LOCALES
        .iter()
        .map(|l| {
            let path = root.join(i18n::DESKTOP_DIR).join(format!("{l}.json"));
            std::fs::read_to_string(&path).map_err(|e| format!("reading {}: {e}", path.display()))
        })
        .collect::<Result<_, _>>()?;
    Ok(i18n::Catalog {
        origin: i18n::DESKTOP_DIR.to_string(),
        files,
    })
}

fn run_i18n(check: bool) -> Result<(), String> {
    let root = root();
    let tag = pinned_tag(&root)?;
    let web = i18n::Catalog {
        origin: format!("web-client {tag} {}", i18n::WEB_DIR),
        files: i18n::LOCALES
            .iter()
            .map(|l| git_show(&root, &tag, &format!("{}/{l}.json", i18n::WEB_DIR)))
            .collect::<Result<_, _>>()?,
    };
    let desktop = desktop_catalog(&root)?;
    let desktop_keys = i18n::keys(&desktop)?;
    let generated = rustfmt(&i18n::generate(&tag, &[web, desktop])?)?;
    let target = root.join(i18n::MESSAGES_RS);
    if !check {
        return std::fs::write(&target, generated)
            .map_err(|e| format!("writing {}: {e}", i18n::MESSAGES_RS));
    }
    let current = std::fs::read_to_string(&target).unwrap_or_default();
    if current != generated {
        return Err(format!(
            "{} is out of date with web-client {tag} and {}; run cargo xtask i18n",
            i18n::MESSAGES_RS,
            i18n::DESKTOP_DIR
        ));
    }
    let unused = i18n::unused(&desktop_keys, &root)?;
    if !unused.is_empty() {
        return Err(format!(
            "desktop messages not used anywhere: {}",
            unused.join(", ")
        ));
    }
    Ok(())
}
