fn main() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../apps/desktop-gpui/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let source = std::fs::read_to_string(manifest).expect("read application release version");
    let version = source.lines().find_map(|line| line.strip_prefix("version = ")).expect("application package version");
    std::fs::write(std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("application-version.rs"), version).expect("write application version");
}
