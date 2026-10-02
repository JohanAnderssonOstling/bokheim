//! Native CPU execution over the shared blocking executor.

use client_platform_runtime::executor;
use cpu_host::{BackendError, BookTask, BoxedCpuReader, CpuHost, CpuRequest, CpuResponse, ThumbnailBytes};
use std::sync::{Arc, Mutex};

pub struct NativeHost {
    pdf: Arc<Mutex<thumbnail::PdfCoverProcess>>,
}
impl Default for NativeHost {
    fn default() -> Self {
        Self { pdf: Arc::new(Mutex::new(thumbnail::PdfCoverProcess::default())) }
    }
}
impl CpuHost for NativeHost {
    fn book(&self, task: BookTask, reader: BoxedCpuReader) -> executor::BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move { executor::run_blocking(move || cpu_host::execute_book_with_mobi(task, reader, |path, bytes| book_inspection::inspect_mobi_pages_reader(path, bytes).map(|book| book.metadata.book).map_err(BackendError::operation))).await.map_err(BackendError::operation)? })
    }
    fn request(&self, request: CpuRequest) -> executor::BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move { executor::run_blocking(move || cpu_host::execute(request)).await.map_err(BackendError::operation)? })
    }
    fn generate_thumbnail(&self, extension: String, reader: BoxedCpuReader) -> executor::BoxedBackendFuture<'_, Result<Option<ThumbnailBytes>, BackendError>> {
        let pdf = self.pdf.clone();
        Box::pin(async move {
            executor::run_blocking(move || {
                thumbnail::generate_thumbnail_versions_with_pdf_renderer(&extension, reader, |reader| pdf.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extract(reader))
                    .map(|versions| versions.map(ThumbnailBytes::from))
                    .map_err(BackendError::operation)
            })
            .await
            .map_err(BackendError::operation)?
        })
    }
}
