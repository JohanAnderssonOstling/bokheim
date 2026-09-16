//! Shared storage coordination and adapters for library services.
//! Domain policy lives in library-files, client-library, and publication-access;
//! platform crates provide the storage mechanisms selected by the host.
pub(crate) mod database;
pub(crate) mod files;
pub(crate) use library_runtime::storage_queue;

pub(crate) mod transfer_history;
