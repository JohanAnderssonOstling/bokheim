//! Integration API for launchers, browser workers, and runtime probes.
//! Views use the clients and result types at the crate root instead.

#[cfg(not(target_arch = "wasm32"))]
pub use crate::api::BackendLaunch;
#[cfg(target_arch = "wasm32")]
pub use crate::runtime::BackendWorkerHost;
pub use crate::runtime::{AppWorkerClientMessage, AppWorkerRequest, AppWorkerStartupMessage, WorkerCommand, WorkerDispatchResult, WorkerPayload, WorkerSubscription, decode_worker_message, encode_worker_message};
#[cfg(not(target_arch = "wasm32"))]
pub use library_registry::LIBRARY_MANIFEST_FILENAME;

/// Browser worker startup, ports, and transport integration.
#[cfg(target_arch = "wasm32")]
pub mod web {
    pub use crate::web::*;
}

/// CPU worker requests and execution.
pub mod cpu {
    pub use cpu_host::{BookTask, CpuRequest, CpuResponse, ThumbnailBytes, execute, execute_book_with_mobi};
}

#[cfg(target_arch = "wasm32")]
pub use crate::runtime::coordinator_lifecycle;

/// Mounted Kobo discovery.
#[cfg(not(target_arch = "wasm32"))]
pub mod kobo {
    pub use client_platform_kobo::{KoboDevice, detect_kobo_devices};
}

/// Gate background side effects during a host-owned update startup trial.
pub use client_platform_runtime::executor::defer_background_work;
