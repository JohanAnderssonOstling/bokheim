use serde::{Deserialize, Serialize};
use bokheim_domain::BookLinkState;
use sync_common::ContentHash;

/// A directed, user-authored relationship between two books in one library.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, specta::Type)]
pub struct BookLink {
    pub id: String,
    pub source_content_hash: String,
    pub target_content_hash: String,
    pub relation: String,
    pub note: String,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub modified_at: i64,
}

impl BookLink {
    pub fn validate(&self, deleted: bool) -> Result<(), Box<dyn std::error::Error>> {
        let source_content_hash = ContentHash::new(&self.source_content_hash.clone());
        let target_content_hash = ContentHash::new(&self.target_content_hash.clone());
        if source_content_hash == target_content_hash {
            return Err("a book link cannot target its own source book".into());
        }
        let _ = BookLinkState {
            source_content_hash,
            target_content_hash,
            relation: self.relation.clone(),
            note: self.note.clone(),
            created_at: u64::try_from(self.created_at)?,
            modified_at: u64::try_from(self.modified_at)?,
            deleted,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> BookLink {
        BookLink {
            id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(),
            source_content_hash: ContentHash::from_fixture_u64(1).to_string(),
            target_content_hash: ContentHash::from_fixture_u64(2).to_string(),
            relation: "contrasts with".to_owned(),
            note: String::new(),
            created_at: 10,
            modified_at: 10,
        }
    }

    #[test]
    fn validates_a_directed_semantic_edge() {
        assert!(link().validate(false).is_ok());
        let mut self_link = link();
        self_link.target_content_hash = self_link.source_content_hash.clone();
        assert!(self_link.validate(false).is_err());
    }
}
