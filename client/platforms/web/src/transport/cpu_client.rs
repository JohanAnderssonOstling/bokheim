//! The synchronous parser worker is owned directly by the storage event loop.
use super::{
    BytesFuture,
    cpu::{CpuService, CpuWorkerOptions},
    endpoint::module_worker,
};
use std::cell::RefCell;
use wasm_bindgen::JsValue;
thread_local! {
    static CPU: RefCell<Option<CpuService>> = const { RefCell::new(None) };
}
pub fn initialize(url: String) {
    CPU.with_borrow_mut(|slot| *slot = Some(CpuService::with_options(move || module_worker(&url, "bokheim-parsing").map(Into::into), CpuWorkerOptions::default())));
}
pub fn request_with_source(bytes: Vec<u8>, source: JsValue) -> Result<BytesFuture, String> {
    CPU.with_borrow(|slot| slot.as_ref().map(|cpu| cpu.request_source(bytes, source)).ok_or_else(|| "The parser worker is not initialized".into()))
}
