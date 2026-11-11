use std::env;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        // System GTK/WebKit resolve their own libc imports at runtime. The
        // helper's direct imports are still audited against glibc 2.17.
        println!("cargo:rustc-link-arg-bin=bokheim-browser=-Wl,--allow-shlib-undefined");
    }
}
