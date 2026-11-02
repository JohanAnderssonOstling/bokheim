//! Async batch dispatch owned by the platform's existing I/O executor.

use crate::fetch_dispatch;

pub(crate) type BatchFetch = fetch_dispatch::Fetch<Vec<(u64, usize)>>;

#[cfg(target_arch = "wasm32")]
pub(crate) use fetch_dispatch::browser;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use fetch_dispatch::native;
