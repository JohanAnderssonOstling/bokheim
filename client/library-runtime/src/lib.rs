//! Process-local services owned by concrete library sessions.

pub mod account;
pub mod database;
pub mod directory;
pub mod events;
pub mod files;
pub mod interest;
pub mod open;
pub mod scheduler;
pub mod session;
pub mod thumbnails;
pub mod transfer_history;
pub mod workers;

pub use client_platform_runtime::executor;
pub use client_platform_runtime::storage_queue;

#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_native::sqlite;
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::sqlite;
