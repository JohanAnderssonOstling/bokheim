//! Book and cover execution, integrity checks, and credential refresh.

pub mod policy;
pub use sync_transport::{download_thumbnail_batch, negotiate_blobs, upload_book_stream, upload_thumbnail_batch, validate_streaming_asset_response, AssetResponseError};

mod error;
pub use error::TransferError;
pub mod books;
pub mod deadline;

pub mod thumbnail_jobs;
pub mod thumbnails;
pub mod transfer;
