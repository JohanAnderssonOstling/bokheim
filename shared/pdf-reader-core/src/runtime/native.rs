use crate::PdfError;
use pdfium_render::prelude::*;
#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;

#[cfg(not(target_os = "windows"))]
fn library_path() -> Result<PathBuf, PdfError> {
    if let Some(path) = std::env::var_os("BOKHEIM_PDFIUM_LIBRARY_PATH") {
        return Ok(path.into());
    }
    if cfg!(test) {
        return Ok(env!("PDFIUM_BUILD_LIBRARY_PATH").into());
    }
    if cfg!(target_os = "android") {
        // Android's app linker namespace resolves native libraries packaged in
        // the APK, including libraries loaded directly from an uncompressed APK.
        return Ok(env!("PDFIUM_LIBRARY_NAME").into());
    }
    if cfg!(feature = "packaged-pdfium") {
        return Ok(env!("PDFIUM_PACKAGED_PATH").into());
    }
    let executable = std::env::current_exe().map_err(PdfError::from)?;
    let directory = executable.parent().ok_or("executable has no parent directory")?;
    let path = if cfg!(target_os = "macos") { directory.join("../Frameworks").join(env!("PDFIUM_LIBRARY_NAME")) } else { directory.join("pdfium").join(env!("PDFIUM_LIBRARY_NAME")) };
    if !path.is_file() {
        return Err(PdfError::new(format!("Packaged PDFium is missing: {}. Build/package the application with its PDFium artifacts.", path.display())));
    }
    Ok(path)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn initialize() -> Result<Pdfium, PdfError> {
    let path = library_path()?;
    Pdfium::bind_to_library(&path).map(Pdfium::new).map_err(|error| PdfError::from_error(&error).context(format!("loading PDFium from {}", path.display())))
}

#[cfg(target_os = "windows")]
pub(crate) fn initialize() -> Result<Pdfium, PdfError> {
    Pdfium::bind_to_statically_linked_library().map(Pdfium::new).map_err(|error| PdfError::from_error(&error).context("initializing static PDFium"))
}
