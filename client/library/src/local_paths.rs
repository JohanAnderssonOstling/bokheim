/// Compatibility re-exports: relative path types live in `library-replica`.
/// New code should import them from `library-replica` (or `library-database`) directly.
pub use library_replica::{
    BookPlacement, RelativeBookPath, RelativeBookPathError, RelativeDirPath, RelativeDirPathError,
};
