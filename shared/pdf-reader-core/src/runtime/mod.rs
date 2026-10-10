#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use native::initialize;
#[cfg(target_arch = "wasm32")]
pub(super) use wasm::initialize;

#[cfg(not(target_arch = "wasm32"))]
pub(super) use crate::native::run;

#[cfg(target_arch = "wasm32")]
pub(super) fn run<T>(work: impl FnOnce() -> crate::PdfResult<T>) -> crate::PdfResult<T> {
    work()
}

/// Native documents live and are closed exclusively on the PDFium worker.
pub(super) type Document = pdfium_render::prelude::PdfDocument<'static>;
