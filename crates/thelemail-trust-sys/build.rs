use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const PACKAGE: &str = "github.com/thelemail/trust/ffi";
const EXPORTS: [&str; 8] = [
    "ThelemailFreeCString",
    "ThelemailKeys",
    "ThelemailMail",
    "ThelemailOpaque",
    "ThelemailProof",
    "ThelemailSession",
    "ThelemailTrust",
    "ThelemailVerifyTlogProof",
];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let module = manifest.join("../../trust");
    let out = PathBuf::from(env::var("OUT_DIR").expect("out dir"));
    let os = env::var("CARGO_CFG_TARGET_OS").expect("target os");
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target arch");

    println!("cargo:rerun-if-changed=build.rs");
    for file in ["go.mod", "go.sum"] {
        println!("cargo:rerun-if-changed={}", module.join(file).display());
    }
    for var in [
        "GO",
        "GOWORK",
        "GOFLAGS",
        "GOPRIVATE",
        "GOPROXY",
        "GOTOOLCHAIN",
        "MACOSX_DEPLOYMENT_TARGET",
        "TRUST_MINGW_CC",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    if let Some(work) = env::var_os("GOWORK").filter(|w| !w.is_empty() && w != "off") {
        let work = PathBuf::from(work);
        println!("cargo:rerun-if-changed={}", work.display());
        for replaced in replaced_modules(&work) {
            println!("cargo:rerun-if-changed={}", replaced.display());
        }
    }

    let goarch = match arch.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        other => panic!("trust: no Go architecture for {other}"),
    };

    match os.as_str() {
        "macos" => build_macos(&module, &out, goarch),
        "windows" => build_windows(&module, &out, goarch),
        "linux" => build_linux(&module, &out, goarch),
        other => panic!("trust: unsupported target os {other}"),
    }
}

fn replaced_modules(work: &Path) -> Vec<PathBuf> {
    let base = work.parent().unwrap_or(Path::new("."));
    fs::read_to_string(work)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_once("=>").map(|(_, target)| target.trim()))
        .filter_map(|target| target.split_whitespace().next())
        .map(|target| base.join(target))
        .filter(|path| path.is_dir())
        .collect()
}

fn go() -> Command {
    let mut cmd = Command::new(env::var("GO").unwrap_or_else(|_| "go".into()));
    cmd.env("CGO_ENABLED", "1");
    cmd
}

fn run(mut cmd: Command, what: &str) {
    let status = cmd
        .status()
        .unwrap_or_else(|err| panic!("trust: {what} could not start: {err}"));
    assert!(status.success(), "trust: {what} failed with {status}");
}

fn output(program: &str, args: &[&str]) -> String {
    let out = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("trust: {program} could not start: {err}"));
    assert!(out.status.success(), "trust: {program} {args:?} failed");
    String::from_utf8(out.stdout)
        .expect("trust: tool output is not utf-8")
        .trim()
        .to_owned()
}

fn testkit() -> bool {
    env::var_os("CARGO_FEATURE_TESTKIT").is_some()
}

fn go_build(module: &Path, mode: &str, artifact: &Path) -> Command {
    let tags = if testkit() {
        "-tags=testexports"
    } else {
        "-tags="
    };
    let mut cmd = go();
    cmd.current_dir(module).args([
        "build",
        "-trimpath",
        tags,
        &format!("-buildmode={mode}"),
        "-o",
    ]);
    cmd.arg(artifact).arg(PACKAGE);
    cmd
}

fn build_macos(module: &Path, out: &Path, goarch: &str) {
    let sdk = output("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);
    let cc = output("xcrun", &["-f", "clang"]);
    let min = env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "13.0".into());
    let clang_arch = if goarch == "arm64" { "arm64" } else { "x86_64" };
    let flags = format!("-isysroot {sdk} -arch {clang_arch} -mmacosx-version-min={min}");

    let mut cmd = go_build(module, "c-archive", &out.join("libthelemail_trust.a"));
    cmd.env("GOOS", "darwin")
        .env("GOARCH", goarch)
        .env("CC", cc)
        .env("CGO_CFLAGS", &flags)
        .env("CGO_LDFLAGS", &flags);
    run(cmd, "go build");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=thelemail_trust");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
    println!("cargo:rustc-link-lib=framework=Security");
    println!("cargo:rustc-link-lib=dylib=resolv");
}

fn build_linux(module: &Path, out: &Path, goarch: &str) {
    let mut cmd = go_build(module, "c-archive", &out.join("libthelemail_trust.a"));
    cmd.env("GOOS", "linux").env("GOARCH", goarch);
    run(cmd, "go build");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=thelemail_trust");
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=resolv");
}

fn build_windows(module: &Path, out: &Path, goarch: &str) {
    assert_eq!(goarch, "amd64", "trust: Windows builds are x86_64 only");
    let cc = env::var("TRUST_MINGW_CC").unwrap_or_else(|_| "x86_64-w64-mingw32-gcc".into());
    let dlltool = cc
        .strip_suffix("gcc")
        .map(|prefix| format!("{prefix}dlltool"))
        .unwrap_or_else(|| "dlltool".into());
    let dll = out.join("thelemail_trust.dll");

    let mut cmd = go_build(module, "c-shared", &dll);
    cmd.env("GOOS", "windows")
        .env("GOARCH", goarch)
        .env("CC", &cc)
        .arg("-ldflags=-extldflags=-static");
    run(cmd, "go build");

    let def = out.join("thelemail_trust.def");
    let mut listing = String::from("LIBRARY thelemail_trust.dll\nEXPORTS\n");
    for name in EXPORTS {
        listing.push_str(name);
        listing.push('\n');
    }
    if testkit() {
        listing.push_str("ThelemailTestkit\n");
    }
    fs::write(&def, listing).expect("trust: write def file");

    let mut lib = Command::new(&dlltool);
    lib.args(["-m", "i386:x86-64", "-d"])
        .arg(&def)
        .arg("-D")
        .arg("thelemail_trust.dll")
        .arg("-l")
        .arg(out.join("thelemail_trust.lib"));
    run(lib, "dlltool");

    let profile = out
        .ancestors()
        .nth(3)
        .expect("trust: OUT_DIR is not inside a cargo profile directory");
    for dir in [profile.to_path_buf(), profile.join("deps")] {
        fs::create_dir_all(&dir).expect("trust: create profile dir");
        fs::copy(&dll, dir.join("thelemail_trust.dll")).expect("trust: copy dll");
    }

    println!("cargo:dll={}", dll.display());
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib=thelemail_trust");
}
