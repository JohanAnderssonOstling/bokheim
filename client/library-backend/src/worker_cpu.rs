//! Platform-neutral CPU conveniences over the shared [`cpu_host`] contract.
//!
//! Execution hosts live in the platform crates (`client-platform-native` and
//! `client-platform-web`). The process-wide host defaults to the current
//! platform; launchers override it with [`set_cpu_host`].

pub use cpu_host::{execute, execute_book, set_cpu_host, BookTask, CpuHost, CpuRequest, CpuResponse, ThumbnailBytes};
pub(crate) use cpu_host::{BookOperation, Cover, HashAbort, HashFinish, HashStart, HashUpdate, Inspect, InspectionCapabilities, Operation, RemoteHashSession, Resize, SubjectOcr, SubjectPages, Thumbnail};
use std::sync::Arc;

/// Native launchers configure and enter the isolated PDF-cover process here.
#[cfg(not(target_arch = "wasm32"))]
pub use thumbnail::{configure_pdf_cover_process, run_pdf_cover_process};

fn default_host() -> Arc<dyn CpuHost> {
    #[cfg(target_arch = "wasm32")]
    {
        Arc::new(client_platform_web::cpu_host::WebHost::default())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Arc::new(client_platform_native::cpu_host::NativeHost::default())
    }
}

pub(crate) fn cpu_host() -> Arc<dyn CpuHost> {
    cpu_host::cpu_host_or_init(default_host)
}

struct HostReader(crate::BoxedBookReader);
impl std::io::Read for HostReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}
impl std::io::Seek for HostReader {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.seek(pos)
    }
}
fn as_cpu_reader(reader: crate::BoxedBookReader) -> cpu_host::BoxedCpuReader {
    Box::new(HostReader(reader))
}

pub(crate) async fn submit_book<C: BookOperation>(operation: C, reader: crate::BoxedBookReader) -> Result<C::Reply, crate::BackendError> {
    cpu_host::submit_book(&*cpu_host(), operation, as_cpu_reader(reader)).await
}

async fn submit<C: Operation>(operation: C) -> Result<C::Reply, crate::BackendError> {
    cpu_host::submit(&*cpu_host(), operation).await
}

pub(crate) async fn resize(bytes: Vec<u8>, width: u32) -> Result<Vec<u8>, crate::BackendError> {
    submit(Resize { bytes, width }).await
}
pub(crate) async fn cover(bytes: Vec<u8>) -> Result<ThumbnailBytes, crate::BackendError> {
    submit(Cover { bytes }).await
}

pub(crate) fn inspection_capabilities() -> InspectionCapabilities {
    cpu_host().capabilities()
}

/// Host-backed thumbnail extraction. The extractor carries the process-wide
/// host, so thumbnail jobs stay platform-neutral.
#[derive(Clone)]
pub(crate) struct ThumbnailExtractor {
    host: Arc<dyn CpuHost>,
}
impl Default for ThumbnailExtractor {
    fn default() -> Self {
        Self { host: cpu_host() }
    }
}
impl asset_transfer::thumbnail_jobs::ThumbnailProcessor for ThumbnailExtractor {
    fn decode(&self, bytes: Vec<u8>) -> crate::executor::BoxedBackendFuture<'_, Result<thumbnail::ThumbnailVersions, crate::BackendError>> {
        let host = self.host.clone();
        Box::pin(async move { cpu_host::cover(&*host, bytes).await.map(|v| thumbnail::ThumbnailVersions { browse: v.browse, high_density: v.high_density }) })
    }
    fn generate(&self, extension: String, reader: crate::BoxedBookReader) -> crate::executor::BoxedBackendFuture<'_, Result<Option<thumbnail::ThumbnailVersions>, crate::BackendError>> {
        let host = self.host.clone();
        Box::pin(async move { host.generate_thumbnail(extension, as_cpu_reader(reader)).await.map(|v| v.map(|v| thumbnail::ThumbnailVersions { browse: v.browse, high_density: v.high_density })) })
    }
}
