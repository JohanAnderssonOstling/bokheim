//! Native filesystem discovery primitives.
//!
//! This module intentionally contains no library database owner, event sink,
//! autonomous scan loop, watcher, or retry policy. `LibrarySession` obtains a
//! database snapshot, calls these pure operations, and conditionally applies
//! the returned data through `library-database`.

mod estimate;
pub mod filesystem_scan;

pub use estimate::estimate_folders_cancellable;
