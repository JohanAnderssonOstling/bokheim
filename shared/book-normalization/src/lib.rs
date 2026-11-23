//! Bibliographic field normalization and filename evidence for ingestion and enrichment.
//!
//! Operates on metadata and strings without reading files or parsing book formats.

mod filename_identifiers;
mod filename_identity;
mod filename_isbn;
mod identity_evidence;
mod title_identity;

pub use filename_identity::unusable_title as unusable_filename_title;
pub use filename_isbn::{from_filename as isbns_from_filename, from_filename_single as isbn_from_filename, record as record_filename_isbns};
pub use title_identity::{normalize, normalize_with_filename, title_readings, unusable_title};
