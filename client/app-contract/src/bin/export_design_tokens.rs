//! Writes `apps/ui/design-tokens/design_tokens.json` from the contract.
//!
//! Mirrors the mobile app's `export_bindings`: `--check` fails instead of
//! writing, so CI can catch a token file that has drifted from the Rust it is
//! supposed to describe.

use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/ui/design-tokens/design_tokens.json");
    let rendered = app_contract::design_tokens::render();

    if std::env::args().any(|argument| argument == "--check") {
        let committed = fs::read_to_string(&path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        if committed != rendered {
            return Err(format!("{} is stale; run `cargo run -p app-contract --bin export_design_tokens`", path.display()).into());
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, rendered)?;
    Ok(())
}
