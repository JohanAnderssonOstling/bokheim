use std::path::PathBuf;

use anyhow::{Context, Result};
use wasm_bindgen_cli_support::Bindgen;

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let input = PathBuf::from(arguments.next().context("missing input WASM path")?);
    let output = PathBuf::from(arguments.next().context("missing output directory")?);
    anyhow::ensure!(arguments.next().is_none(), "unexpected extra argument");

    let mut bindgen = Bindgen::new();
    bindgen
        .input_path(&input)
        .out_name("bokheim_web_core")
        .web(true)?
        .typescript(false)
        .generate(&output)
        .with_context(|| format!("could not generate browser bindings for {}", input.display()))
}
