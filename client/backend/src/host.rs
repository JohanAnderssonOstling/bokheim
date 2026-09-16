//! Integration API for launchers, browser workers, and runtime probes.
//! Views use the clients and result types at the crate root instead.

#[cfg(not(target_arch = "wasm32"))]
pub use crate::api::BackendLaunch;
#[cfg(not(target_arch = "wasm32"))]
pub use crate::platform::kobo::LIBRARY_MANIFEST_FILENAME;
#[cfg(target_arch = "wasm32")]
pub use crate::runtime::BackendWorkerHost;
pub use crate::runtime::{AppWorkerClientMessage, AppWorkerRequest, AppWorkerStartupMessage, ResolvedBookData, WorkerCommand, WorkerDispatchResult, WorkerPayload, WorkerSubscription, decode_worker_message, encode_worker_message};
#[cfg(target_arch = "wasm32")]
pub fn subscribe_web_diagnostics() -> async_channel::Receiver<String> {
    let (sender, receiver) = async_channel::unbounded();
    crate::WEB_DIAGNOSTICS.with(|subscribers| subscribers.borrow_mut().push(sender));
    receiver
}

/// Browser worker startup, ports, and transport integration.
#[cfg(target_arch = "wasm32")]
pub mod web {
    pub use crate::browser_adapter::*;
}

/// CPU worker requests and execution.
pub mod cpu {
    pub use crate::worker_cpu::{CpuRequest, CpuResponse, PublicationTask, ThumbnailBytes, execute, execute_publication};
    #[cfg(not(target_arch = "wasm32"))]
    pub use thumbnail::{configure_pdf_cover_process, run_pdf_cover_process};
}

#[cfg(all(target_arch = "wasm32", feature = "web-runtime-tests"))]
pub mod sync_contract {
    pub use crate::worker_sync::run;
}

/// Runtime probes and opt-in storage contracts.
pub mod testing {
    #[cfg(all(target_arch = "wasm32", feature = "storage-contract-tests"))]
    pub use crate::contracts::{storage_contract, sync_contract};
    pub use crate::executor::sleep;
    #[cfg(feature = "web-runtime-tests")]
    pub use crate::sync::timing::BackgroundTiming;
    #[cfg(all(target_arch = "wasm32", feature = "web-runtime-tests"))]
    pub use crate::{app::notification_socket_contract, executor::timer_lifecycle_contract};
}

#[cfg(target_arch = "wasm32")]
pub use crate::runtime::coordinator_lifecycle;
