//! Native CPU execution over the shared blocking executor.

use crate::{execute, execute_book_with_mobi, BackendError, BookTask, BoxedCpuReader, CpuHost, CpuRequest, CpuResponse};
use book_inspection::inspect_mobi_pages_reader;
use client_platform_runtime::executor::{inspection_permit, run_blocking, BoxedBackendFuture};

#[derive(Default)]
pub struct NativeHost;
impl CpuHost for NativeHost {
    fn book(&self, task: BookTask, reader: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move {
            let permit = inspection_permit().await;
            run_blocking(move || {
                let _permit = permit;
                execute_book_with_mobi(task, reader, |path, bytes| inspect_mobi_pages_reader(path, bytes).map(|book| book.metadata.book).map_err(BackendError::operation))
            })
            .await
            .map_err(BackendError::operation)?
        })
    }
    fn request(&self, request: CpuRequest) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move { run_blocking(move || execute(request)).await.map_err(BackendError::operation)? })
    }
}
