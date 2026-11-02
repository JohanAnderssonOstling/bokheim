//! Application command dispatch and native/browser transport.
//!
//! Browser hosts keep SQLite and its synchronous OPFS VFS behind an
//! origin-wide shared worker and use a serialized RPC protocol. Native hosts
//! send concrete commands to one application-owned backend thread.

use sync_common::LibraryId;
use tokio::sync::oneshot;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::Transport;
mod protocol;
pub(crate) mod requests;
pub(crate) use client_runtime::reply::WireReply;
pub use requests::AppCommand;
#[cfg(target_arch = "wasm32")]
pub(crate) mod bisac;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(target_arch = "wasm32")]
pub(crate) use web::Transport;
#[cfg(any(target_arch = "wasm32", test))]
mod web_state;

use crate::api::AppClient;
pub use protocol::*;

#[cfg(target_arch = "wasm32")]
mod host;
#[cfg(target_arch = "wasm32")]
pub(crate) mod taxonomy;
#[cfg(target_arch = "wasm32")]
pub use host::BackendWorkerHost;
