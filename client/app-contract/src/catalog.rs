//! Catalog application API plus the client-owned library ownership overlay.

pub use catalog_contract::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct CatalogPublicationKey {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ownership_keys_include_the_complete_publication_identity() {
        let source_id = CatalogSourceId::parse("opds-test").unwrap();
        let first = CatalogPublicationKey { source_id: source_id.clone(), publication: CatalogPublicationIdentity { edition_id: "edition".to_owned(), work_id: Some("work-one".to_owned()) } };
        let second = CatalogPublicationKey { source_id, publication: CatalogPublicationIdentity { edition_id: "edition".to_owned(), work_id: Some("work-two".to_owned()) } };

        assert_eq!(HashSet::from([first, second]).len(), 2);
    }
}
