use crate::PdfError;
use pdfium_render::prelude::*;

pub(crate) fn initialize() -> Result<Pdfium, PdfError> {
    // The worker initializes the separately packaged Emscripten module first.
    Pdfium::bind_to_system_library().map(Pdfium::new).map_err(|error| PdfError::from_error(&error).context("binding PDFium WASM"))
}
