use book_model::{BookRecord, BookTocEntry};
use metadata_contract::WorkPopularity;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A definitive enrichment answer. Failures are device-local retry work.
#[derive(Debug, Clone, PartialEq)]
pub enum EnrichmentResult {
    Found { record: BookRecord, toc: Option<Vec<BookTocEntry>>, cover_id: Option<String>, popularity: Option<WorkPopularity> },
    NoMatch,
}

// Retain the two original positional variants so saved additions remain readable.
// The appended variant carries popularity without changing their binary layout.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Encoded<R, T, C, P> {
    Found { record: R, toc: T, cover_id: C },
    NoMatch,
    FoundWithPopularity { record: R, toc: T, cover_id: C, popularity: P },
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "EnrichmentResult", rename_all = "snake_case")]
enum HumanEnrichment {
    Found {
        record: BookRecord,
        toc: Option<Vec<BookTocEntry>>,
        cover_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        popularity: Option<WorkPopularity>,
    },
    NoMatch,
}

impl Serialize for EnrichmentResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return HumanEnrichment::serialize(self, serializer);
        }
        let encoded = match self {
            Self::Found { record, toc, cover_id, popularity: None } => Encoded::Found { record, toc, cover_id },
            Self::Found { record, toc, cover_id, popularity: Some(popularity) } => Encoded::FoundWithPopularity { record, toc, cover_id, popularity },
            Self::NoMatch => Encoded::NoMatch,
        };
        encoded.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EnrichmentResult {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            return HumanEnrichment::deserialize(deserializer);
        }
        Ok(match Encoded::deserialize(deserializer)? {
            Encoded::Found { record, toc, cover_id } => Self::Found { record, toc, cover_id, popularity: None },
            Encoded::FoundWithPopularity { record, toc, cover_id, popularity } => Self::Found { record, toc, cover_id, popularity: Some(popularity) },
            Encoded::NoMatch => Self::NoMatch,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The schema used before popularity was added, independent of the adapter.
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum LegacyEnrichment {
        Found { record: BookRecord, toc: Option<Vec<BookTocEntry>>, cover_id: Option<String> },
        NoMatch,
    }

    #[test]
    fn original_saved_answers_decode_and_keep_their_binary_layout() {
        let record = BookRecord { metadata: Default::default(), description: String::new() };
        let original = LegacyEnrichment::Found { record: record.clone(), toc: None, cover_id: None };
        let bytes = sync_common::wire::encode(&original).unwrap();
        let result: EnrichmentResult = sync_common::wire::decode(&bytes, 4096).unwrap();
        assert_eq!(result, EnrichmentResult::Found { record, toc: None, cover_id: None, popularity: None });
        assert_eq!(sync_common::wire::encode(&result).unwrap(), bytes);
        let transport = sync_common::transport::encode(&original).unwrap();
        assert_eq!(sync_common::transport::decode::<EnrichmentResult>(&transport, 4096).unwrap(), result);
        assert_eq!(sync_common::wire::encode(&EnrichmentResult::NoMatch).unwrap(), sync_common::wire::encode(&LegacyEnrichment::NoMatch).unwrap());
    }

    #[test]
    fn popularity_survives_storage_and_sync_encodings() {
        let result = EnrichmentResult::Found {
            record: BookRecord { metadata: Default::default(), description: String::new() },
            toc: None,
            cover_id: None,
            popularity: Some(WorkPopularity { open_library_work_id: "OL10W".into(), rating_count: 12, snapshot_date: "2026-09-01".into() }),
        };
        let bytes = sync_common::wire::encode(&result).unwrap();
        assert_eq!(sync_common::wire::decode::<EnrichmentResult>(&bytes, 4096).unwrap(), result);
        let bytes = sync_common::transport::encode(&result).unwrap();
        assert_eq!(sync_common::transport::decode::<EnrichmentResult>(&bytes, 4096).unwrap(), result);
        assert!(matches!(sync_common::transport::decode::<LegacyEnrichment>(&bytes, 4096).unwrap(), LegacyEnrichment::Found { .. }));
    }
}
