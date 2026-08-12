//! Capabilities scoped to the selected library.

mod context;
mod latest_request;

pub(crate) use context::{AuthorBrowseRows, HomeBrowseRows, LibraryContext};
pub(crate) use latest_request::LatestRequest;
