//! Integration API for launchers, browser workers, and host probes.
//! Views use the clients and result types at the crate root instead.

#[cfg(not(target_arch = "wasm32"))]
pub use crate::api::BackendLaunch;
#[cfg(not(target_arch = "wasm32"))]
pub use library_registry::LIBRARY_MANIFEST_FILENAME;

/// Browser worker startup, ports, and transport integration.
#[cfg(target_arch = "wasm32")]
pub mod web {
    pub use crate::web::*;
}

/// CPU worker requests and execution.
pub mod cpu {
    #[cfg(target_arch = "wasm32")]
    pub use crate::web::{initialize_bisac_reference, initialize_bundled_taxonomy};
    pub use cpu_host::{BookTask, CpuRequest, CpuResponse, ThumbnailBytes, execute, execute_book_with_mobi};
}

/// Mounted Kobo discovery.
#[cfg(not(target_arch = "wasm32"))]
pub mod kobo {
    pub use client_platform_kobo::{KoboDevice, detect_kobo_devices};
}

/// Gate background side effects during a host-owned update startup trial.
pub use client_platform_runtime::executor::defer_background_work;
#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_runtime::transfers;
