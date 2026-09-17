//! Native CPU execution and the generation batch's PDF subprocess.
use super::*;

pub(super) async fn book(task: BookTask, reader: crate::BoxedBookReader) -> Result<CpuResponse, String> {
    crate::executor::run_blocking(move || execute_book(task, reader)).await.map_err(|error| error.to_string())?
}

pub(super) async fn request(request: CpuRequest) -> Result<CpuResponse, String> {
    crate::executor::run_blocking(move || execute(request)).await.map_err(|error| error.to_string())?
}

pub(super) const CAPABILITIES: InspectionCapabilities = InspectionCapabilities { ocr: true, mobi_pages: true };

pub(super) fn mobi_pages(path: &std::path::Path, reader: crate::BoxedBookReader) -> Result<book_model::BookMetadata, String> {
    book_inspection::inspect_mobi_pages_reader(path, reader).map(|book| book.book).map_err(|error| error.to_string())
}

#[derive(Clone, Default)]
pub(crate) struct ThumbnailExtractor {
    pdf: std::sync::Arc<std::sync::Mutex<thumbnail::PdfCoverProcess>>,
}
impl ThumbnailExtractor {
    pub(crate) async fn generate(&self, extension: String, reader: crate::BoxedBookReader) -> Result<Option<ThumbnailBytes>, String> {
        let pdf = self.pdf.clone();
        crate::executor::run_blocking(move || {
            thumbnail::generate_thumbnail_versions_with_pdf_renderer(&extension, reader, |reader| pdf.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extract(reader))
                .map(|versions| versions.map(ThumbnailBytes::from))
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
