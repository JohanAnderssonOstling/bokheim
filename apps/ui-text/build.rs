use std::{collections::BTreeMap, env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=english.json");
    let source = fs::read_to_string("english.json").expect("read UI text catalog");
    let catalog: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(&source).expect("parse UI text catalog");
    let mut generated = String::from("#[macro_export]\nmacro_rules! text {\n");
    for (section, entries) in catalog {
        assert!(identifier(&section), "invalid UI text section: {section}");
        for (key, value) in entries {
            assert!(identifier(&key), "invalid UI text key: {section}.{key}");
            generated.push_str(&format!("    ({section}::{key}) => {{ {value:?} }};\n"));
        }
    }
    generated.push_str("}\n");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    fs::write(output.join("text.rs"), generated).expect("write UI text macro");
}

fn identifier(value: &str) -> bool {
    !value.is_empty() && value.bytes().enumerate().all(|(index, byte)| byte.is_ascii_lowercase() || byte == b'_' || (index > 0 && byte.is_ascii_digit()))
}
