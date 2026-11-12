use book_model::BookTocEntry;
use serde::{Deserialize, Serialize};

/// A definitive lookup outcome; its metadata is already applied to the book.
/// Failures are device-local retry work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "StoredEnrichment", into = "StoredEnrichment")]
pub enum EnrichmentResult {
    Found {
        #[serde(with = "book_model::toc_transport::optional")]
        toc: Option<Vec<BookTocEntry>>,
        cover_id: Option<String>,
    },
    NoMatch,
}

// Keep the existing binary slot readable without exposing or using retired scores.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RetiredScore {
    open_library_work_id: String,
    rating_count: u64,
    snapshot_date: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StoredEnrichment {
    Found {
        #[serde(with = "book_model::toc_transport::optional")]
        toc: Option<Vec<BookTocEntry>>,
        cover_id: Option<String>,
        popularity: Option<RetiredScore>,
    },
    NoMatch,
}

impl From<StoredEnrichment> for EnrichmentResult {
    fn from(value: StoredEnrichment) -> Self {
        match value {
            StoredEnrichment::Found { toc, cover_id, .. } => Self::Found { toc, cover_id },
            StoredEnrichment::NoMatch => Self::NoMatch,
        }
    }
}

impl From<EnrichmentResult> for StoredEnrichment {
    fn from(value: EnrichmentResult) -> Self {
        match value {
            EnrichmentResult::Found { toc, cover_id } => Self::Found { toc, cover_id, popularity: None },
            EnrichmentResult::NoMatch => Self::NoMatch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::BookRecord;

    fn draft() -> crate::BookAddition {
        let mut record = BookRecord { metadata: Default::default(), description: "File description".into() };
        record.title = "File title".into();
        record.subtitle = Some("File subtitle".into());
        record.book.year = Some(1999);
        crate::BookAddition { facts: crate::BookFacts { added_at: 1, format: book_model::BookFormat::Epub }, record, navigation: None, pdf: None, recording: None, enrichment: None }
    }

    fn provider() -> BookRecord {
        use book_model::{AUTHOR_MARC_RELATOR_CODE, BookSubject, Contributor, ContributorId};
        let mut record = BookRecord { metadata: Default::default(), description: "Provider description".into() };
        record.title = "Provider title".into();
        record.subtitle = Some("Provider subtitle".into());
        record.book.year = Some(2005);
        record.book.publishers = vec!["Provider publisher".into()];
        record.book.subjects = vec![BookSubject::new(None, "Provider subject", "provider", None, None).unwrap()];
        record.contributors = vec![Contributor::with_id(ContributorId::from_u128(1), "Provider author", AUTHOR_MARC_RELATOR_CODE).unwrap()];
        record
    }

    fn assert_effective_record(addition: &crate::BookAddition, description: &str, year: i32) {
        assert_eq!(addition.record.title, "File title");
        assert_eq!(addition.record.subtitle.as_deref(), Some("File subtitle"));
        assert_eq!(addition.record.description, description);
        assert_eq!(addition.record.book.year, Some(year));
        assert_eq!(addition.record.book.publishers, ["Provider publisher"]);
        assert_eq!(addition.record.book.subjects[0].name(), "Provider subject");
        assert_eq!(addition.record.contributors[0].name(), "Provider author");
    }

    #[test]
    fn enrichment_updates_one_record_and_preserves_file_precedence() {
        for has_file_values in [false, true] {
            let mut addition = draft();
            if !has_file_values {
                addition.record.description = " \n".into();
                addition.record.book.year = None;
            }
            addition.apply_enrichment(provider(), EnrichmentResult::Found { toc: None, cover_id: Some("cover".into()) });
            assert_effective_record(&addition, if has_file_values { "File description" } else { "Provider description" }, if has_file_values { 1999 } else { 2005 });
            let json = serde_json::to_value(&addition).unwrap();
            assert!(json["enrichment"]["found"].get("record").is_none());
            assert_eq!(serde_json::from_value::<crate::BookAddition>(json).unwrap(), addition);
            let persisted = sync_common::wire::encode(&addition).unwrap();
            assert_eq!(sync_common::wire::decode::<crate::BookAddition>(&persisted, 100_000).unwrap(), addition);
            let transport = sync_common::transport::encode(&addition).unwrap();
            assert_eq!(sync_common::transport::decode::<crate::BookAddition>(&transport, 100_000).unwrap(), addition);
            assert_eq!(sync_common::transport::representation::<_, crate::BookAddition>(&addition).unwrap(), addition);
        }
    }

    #[test]
    fn unanswered_drafts_and_no_match_keep_their_original_record() {
        let mut addition = draft();
        for result in [None, Some(EnrichmentResult::NoMatch)] {
            addition.enrichment = result;
            let bytes = sync_common::wire::encode(&addition).unwrap();
            assert_eq!(sync_common::wire::decode::<crate::BookAddition>(&bytes, 100_000).unwrap(), addition);
        }
    }
}
