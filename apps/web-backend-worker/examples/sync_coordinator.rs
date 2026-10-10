#![cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;


#[wasm_bindgen(js_name = startNetworkWorker)]
pub fn start_network_worker() -> Result<(), JsValue> {
    app::host::web::network_server::start()
}

#[wasm_bindgen(js_name = startSharedCoordinator)]
pub fn start_shared_coordinator() -> Result<(), JsValue> {
    app::host::web::network_client::install()?;
    let coordinator = hosts::SharedCoordinator::new();
    let failed = coordinator.clone();
    client_platform_web::transport::scope::shared_connections(move |message| failed.fail(&message), move |port| coordinator.connect(port))
}


#[path = "../src/coordinator.rs"]
mod coordinator;
#[path = "../src/hosts.rs"]
mod hosts;
use app::host::web::{BackgroundService, SyncService};
use client_platform_web::transport::{ClientRelay, byte_reply, cpu::CpuService, error_message, failure, field, network_client, network_server, object};
use coordinator::Coordinator;

/// Application worker topology configuration.
#[derive(Clone, Copy, Debug)]
struct WorkerOptions {
    cpu: client_platform_web::transport::cpu::CpuWorkerOptions,
    sync: app::host::web::SyncOptions,
}
impl Default for WorkerOptions {
    fn default() -> Self {
        Self { cpu: Default::default(), sync: Default::default() }
    }
}
