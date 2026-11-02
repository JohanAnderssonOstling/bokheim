#[path = "src/fonts/bundled.rs"]
mod bundled;

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    println!("cargo:rerun-if-changed=src/fonts/bundled.rs");
    println!("cargo:rerun-if-changed=../desktop-gpui/assets/fonts");
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") || env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        return;
    }
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../desktop-gpui/assets/fonts");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let destination = out.ancestors().nth(3).expect("Cargo profile directory").join("fonts");
    fs::create_dir_all(&destination).expect("create bundled font directory");
    for family in bundled::FAMILIES {
        for name in family.files {
            let bytes = fs::read(source.join(name)).unwrap_or_else(|error| panic!("read bundled {} font {name}: {error}", family.name));
            publish(&destination.join(name), &bytes);
        }
    }
    for entry in fs::read_dir(&source).expect("list font notices") {
        let entry = entry.expect("read font notice entry");
        if entry.file_name().to_string_lossy().starts_with("LICENSE") {
            publish(&destination.join(entry.file_name()), &fs::read(entry.path()).expect("read font notice"));
        }
    }
}

fn publish(destination: &Path, bytes: &[u8]) {
    if fs::read(destination).ok().as_deref() != Some(bytes) {
        let staging = destination.with_extension(format!("tmp-{}", std::process::id()));
        fs::write(&staging, bytes).expect("stage bundled font");
        // Windows cannot rename over an existing file.
        #[cfg(windows)]
        if destination.exists() {
            fs::remove_file(destination).expect("replace bundled font");
        }
        fs::rename(staging, destination).expect("publish bundled font");
    }
}
