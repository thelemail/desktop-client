use std::path::Path;

fn main() {
    bundle_trust_library();
    let dist = Path::new("../../web-client/build");
    println!("cargo:rerun-if-changed={}", dist.display());
    watch(dist);
    tauri_build::build()
}

fn bundle_trust_library() {
    println!("cargo:rerun-if-env-changed=DEP_THELEMAIL_TRUST_DLL");
    let Ok(dll) = std::env::var("DEP_THELEMAIL_TRUST_DLL") else {
        return;
    };
    std::fs::create_dir_all("trust-lib").expect("trust: create the bundle directory");
    std::fs::copy(&dll, "trust-lib/thelemail_trust.dll")
        .expect("trust: stage the library for bundling");
}

fn watch(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        println!("cargo:rerun-if-changed={}", path.display());
        if path.is_dir() {
            watch(&path);
        }
    }
}
