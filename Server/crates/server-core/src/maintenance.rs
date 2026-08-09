#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GcReport {
    pub blobs_scanned: usize,
    pub blobs_deleted: usize,
    pub thumbnails_scanned: usize,
    pub thumbnails_deleted: usize,
    pub bytes_reclaimed: u64,
}
