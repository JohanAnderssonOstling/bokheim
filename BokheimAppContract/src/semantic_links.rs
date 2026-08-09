use serde::{Deserialize, Serialize};
use bokheim_domain::BookLinkState;
use sync_common::{BookId, UnixMillis};

/// A directed, user-authored relationship between two books in one library.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, specta::Type)]
pub struct BookLink {
    pub id: String,
    pub source_book_id: String,
    pub target_book_id: String,
    pub relation: String,
    pub note: String,
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
    #[specta(type = specta_typescript::Number)]
    pub modified_at: i64,
}

impl BookLink {
    pub fn validate(&self, deleted: bool) -> Result<(), Box<dyn std::error::Error>> {
        let source_book_id = BookId::parse(&self.source_book_id)?;
        let target_book_id = BookId::parse(&self.target_book_id)?;
        if source_book_id.canonical() != self.source_book_id || target_book_id.canonical() != self.target_book_id {
            return Err("book link ids must use canonical decimal strings".into());
        }
        if source_book_id == target_book_id {
            return Err("a book link cannot target its own source book".into());
        }
        let _ = BookLinkState {
            source_book_id,
            target_book_id,
            relation: self.relation.clone(),
            note: self.note.clone(),
            created_at: UnixMillis::new(u64::try_from(self.created_at)?)?,
            modified_at: UnixMillis::new(u64::try_from(self.modified_at)?)?,
            deleted,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link() -> BookLink {
        BookLink { id: "9f58fba8-f7d8-4f87-8e1d-5fe5b82f2a21".to_owned(), source_book_id: "1".to_owned(), target_book_id: "2".to_owned(), relation: "contrasts with".to_owned(), note: String::new(), created_at: 10, modified_at: 10 }
    }

    #[test]
    fn validates_a_directed_semantic_edge() {
        assert!(link().validate(false).is_ok());
        let mut self_link = link();
        self_link.target_book_id = self_link.source_book_id.clone();
        assert!(self_link.validate(false).is_err());
        let mut noncanonical = link();
        noncanonical.source_book_id = "01".to_owned();
        assert!(noncanonical.validate(false).is_err());
    }
}
