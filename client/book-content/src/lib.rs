//! Parsed EPUB and MOBI containers, metadata, resources, and reading preparation.
//! The crate has no renderer dependency; consumers adapt its prepared resources.

mod book;
mod epub;
mod inspection;
mod metadata;
pub mod mobi;
mod resource_path;
mod resources;
mod source;
mod toc;

pub use book::{Book, BookFormat, ReadingBook};
pub use inspection::DocumentInspection;
pub use metadata::{CreatorRole, DeclaredMetadata, IdentifierScheme, MetadataCredit, MetadataDate, MetadataIdentifier, MetadataIdentifierType, MetadataProperty, MetadataSubject, MetadataTitle};
pub use resource_path::resolve_resource_href;
pub use resources::PreparedResources;
pub use source::BookReader;
pub use toc::TocEntry;
