use std::path::Path;

fn main() {
    let dist = Path::new("../../web-client/build");
    println!("cargo:rerun-if-changed={}", dist.display());
    watch(dist);
    tauri_build::build()
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
