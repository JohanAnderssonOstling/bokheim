use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bindings_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/lib/bindings.ts");
    if std::env::args().any(|argument| argument == "--check") {
        let temporary_path = std::env::temp_dir().join(format!("mobileapp-bindings-{}.ts", std::process::id()));
        mobileapp_lib::export_bindings(&temporary_path)?;
        let generated = fs::read(&temporary_path)?;
        fs::remove_file(&temporary_path)?;
        let committed = fs::read(&bindings_path).map_err(|error| format!("failed to read {}: {error}", bindings_path.display()))?;
        if generated != committed {
            return Err(format!("{} is stale; run `npm run generate:bindings`", bindings_path.display()).into());
        }
    } else {
        mobileapp_lib::export_bindings(&bindings_path)?;
    }
    Ok(())
}
