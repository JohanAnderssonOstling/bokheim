//! Native CPU execution over the shared blocking executor.

use crate::{BackendError, BookTask, BoxedCpuReader, CpuHost, CpuRequest, CpuResponse, execute, execute_book_with_mobi};
use book_inspection::inspect_mobi_frontmatter;
use client_platform_runtime::executor::{BoxedBackendFuture, inspection_permit, run_blocking};
use client_platform_runtime::profiling::{ProfileIo, ProfileSpan};

#[derive(Default)]
pub struct NativeHost;
impl CpuHost for NativeHost {
    fn book(&self, task: BookTask, reader: BoxedCpuReader) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move {
            let label = match &task {
                BookTask::Inspect { source_name, .. } | BookTask::InspectEpubMetadata { source_name } => Some(source_name.clone()),
                _ => None,
            };
            let waiting = label.as_deref().map(|label| ProfileSpan::new("parser_permit_wait", label));
            let permit = inspection_permit().await;
            drop(waiting);
            let queued = label.as_deref().map(|label| ProfileSpan::new("parser_worker_queue", label));
            run_blocking(move || {
                drop(queued);
                let _permit = permit;
                let profile = label.as_deref().map(|label| ProfileSpan::new("metadata_parse", label));
                let reader = ProfileIo::new(reader, profile.as_ref().and_then(ProfileSpan::io));
                execute_book_with_mobi(task, reader, |book, text| inspect_mobi_frontmatter(book.clone(), text).map(|book| book.metadata.book).map_err(BackendError::operation))
            })
            .await
            .map_err(BackendError::operation)?
        })
    }
    fn request(&self, request: CpuRequest) -> BoxedBackendFuture<'_, Result<CpuResponse, BackendError>> {
        Box::pin(async move { run_blocking(move || execute(request)).await.map_err(BackendError::operation)? })
    }
}
