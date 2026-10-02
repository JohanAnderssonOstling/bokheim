//! Asset transfer jobs, scheduling, retry policy, and progress aggregation.

pub mod policy;
pub use library_runtime::{ClaimedTransferJob, HistoryPersistence, MessagePackTransferHistory, TransferJob, TransferOperation, TransferProducer, TransferQueue};
pub use sync_transport::{download_thumbnail_batch, negotiate_blobs, upload_book_revision_stream, upload_thumbnail_batch, validate_streaming_asset_response, AssetResponseError};

mod error;
pub use error::TransferError;
pub mod books;
pub mod deadline;

pub mod planning;
pub mod thumbnail_jobs;
pub mod thumbnails;
pub mod transfer;

mod concurrent_uploads;
pub mod runner;
