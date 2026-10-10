#![cfg(not(target_arch = "wasm32"))]
#[path = "asset_workflow/import_workflow.rs"]
mod import_workflow;
#[path = "asset_workflow/ingestion.rs"]
mod ingestion;
