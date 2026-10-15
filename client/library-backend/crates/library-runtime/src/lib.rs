//! Process-local services owned by concrete library sessions.

pub mod account;
pub mod events;
pub mod interest;
pub mod reading_positions;
pub mod scheduler;

pub use client_platform_runtime::executor;
pub use client_platform_runtime::storage_queue;
pub use client_platform_runtime::transfers;

#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_native::sqlite;
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::sqlite;
