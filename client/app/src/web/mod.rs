//! Browser adapters for backend services. Application worker topology lives in
//! apps/web-backend-worker; reusable browser mechanisms live in client-platform-web.
#![cfg(target_arch = "wasm32")]

mod background;
mod tab_host;
use client_platform_web::transport::cpu;
pub use client_platform_web::transport::{BytesFuture, BytesOperation};
pub use tab_host::{WebConfiguration, WebConnection};
pub use client_platform_web::transport::network_client;
pub use client_platform_web::transport::network_server;
mod sync;
pub use {
    background::BackgroundService,
    cpu::CpuService,
    sync::{SyncOptions, SyncService},
};

use client_platform_web::transport::{Port, byte_reply, error_message};

pub(crate) use client_platform_web::transport::object;
