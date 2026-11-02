use serde::Deserialize;
use std::{env, error::Error, fs, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Protocol {
    version: u32,
    media_type: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=protocol.json");
    let protocol: Protocol = serde_json::from_str(&fs::read_to_string("protocol.json")?)?;
    if protocol.version == 0 || !protocol.media_type.contains('/') || protocol.media_type.chars().any(|c| c.is_whitespace() || matches!(c, ';' | '\r' | '\n')) {
        return Err("invalid protocol version or base media type in protocol.json".into());
    }
    let media_type = format!("{}; version={}", protocol.media_type, protocol.version);
    let constants = format!("pub const MEDIA_TYPE: &str = {media_type:?};\npub const MEDIA_TYPE_BASE: &str = {base:?};\nconst VERSION: u32 = {version};\n", base = protocol.media_type, version = protocol.version,);
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("protocol.rs"), constants)?;
    Ok(())
}
