//! Integration API for launchers, browser workers, and runtime probes.
//! Views use the clients and result types at the crate root instead.

#[cfg(not(target_arch = "wasm32"))]
pub use crate::api::BackendLaunch;
#[cfg(not(target_arch = "wasm32"))]
pub use library_registry::LIBRARY_MANIFEST_FILENAME;
#[cfg(target_arch = "wasm32")]
pub use crate::runtime::BackendWorkerHost;
pub use crate::runtime::{decode_worker_message, encode_worker_message, AppWorkerClientMessage, AppWorkerRequest, AppWorkerStartupMessage, ResolvedBookData, WorkerCommand, WorkerDispatchResult, WorkerPayload, WorkerSubscription};

/// Browser worker startup, ports, and transport integration.
#[cfg(target_arch = "wasm32")]
pub mod web {
    pub use crate::web::*;
}

/// CPU worker requests and execution.
pub mod cpu {
    pub use crate::worker_cpu::{execute, execute_book, BookTask, CpuRequest, CpuResponse, ThumbnailBytes};
    #[cfg(not(target_arch = "wasm32"))]
    pub use library_backend::worker_cpu::{configure_pdf_cover_process, run_pdf_cover_process};
}

#[cfg(all(target_arch = "wasm32", feature = "web-runtime-tests"))]
pub mod sync_contract {
    pub use crate::sync::run;
}

/// Runtime probes and opt-in storage contracts.
pub mod testing {
    #[cfg(all(target_arch = "wasm32", feature = "storage-contract-tests"))]
    pub use crate::contracts::{storage_contract, sync_contract};
    pub use crate::executor::sleep;
    #[cfg(feature = "web-runtime-tests")]
    pub use crate::sync::BackgroundTiming;
    #[cfg(all(target_arch = "wasm32", feature = "web-runtime-tests"))]
    pub use crate::{app::notification_socket_contract, executor::timer_lifecycle_contract};
}

#[cfg(target_arch = "wasm32")]
pub use crate::runtime::coordinator_lifecycle;

/// Mounted Kobo discovery and application-owned library transfers.
#[cfg(not(target_arch = "wasm32"))]
pub mod kobo {
    pub use client_platform_kobo::{detect_kobo_devices, KoboDevice};
    pub use crate::kobo_transfer::{transfer_libraries_to_kobo, KoboTransferSummary};
}
