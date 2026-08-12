use crate::app::BackendError;
use crate::library::database as db;
use crate::library::LibraryBackend;
use crate::ContentHash;
use book_metadata::external::{ExternalClassificationScheme, ExternalMetadataProviders, ExternalSubjectScheme};
use book_metadata::library_of_congress::LibraryOfCongressProvider;
use library_model::PublicationSubject;

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct MissingSubjectsBook {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub title: String,
    pub isbn: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SubjectEnrichmentStatus {
    Updated,
    NoMatch,
    Ambiguous,
    Failed,
}

impl SubjectEnrichmentStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::NoMatch => "no_match",
            Self::Ambiguous => "ambiguous",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct SubjectEnrichmentOutcome {
    #[specta(type = String)]
    pub content_hash: ContentHash,
    pub provider_id: String,
    pub status: SubjectEnrichmentStatus,
    pub imported_subjects: usize,
    pub detail: Option<String>,
}

impl LibraryBackend {
    pub fn books_missing_subjects(&self, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<MissingSubjectsBook>, BackendError> {
        let connection = self.db.get().map_err(BackendError::operation)?;
        let candidates = db::subject_enrichment_candidates(&connection, provider_id, limit, retry_failed).map_err(BackendError::operation)?;
        Ok(candidates.into_iter().map(|candidate| MissingSubjectsBook { content_hash: candidate.content_hash, title: candidate.title, isbn: candidate.isbn }).collect())
    }

    /// Enriches books in provider order. Once a provider adds subjects, later
    /// providers no longer see that book as missing; failures and empty results
    /// remain eligible for the next provider but are not retried automatically.
    pub async fn enrich_missing_subjects(&self, providers: &ExternalMetadataProviders, limit_per_provider: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentOutcome>, BackendError> {
        let mut outcomes = Vec::new();
        for provider_id in providers.provider_ids() {
            let candidates = {
                let connection = self.db.get().map_err(BackendError::operation)?;
                db::subject_enrichment_candidates(&connection, provider_id.as_str(), limit_per_provider, retry_failed).map_err(BackendError::operation)?
            };
            eprintln!("SUBJECT_ENRICHMENT provider={} candidates={}", provider_id.as_str(), candidates.len());
            for candidate in candidates {
                eprintln!("SUBJECT_ENRICHMENT lookup provider={} isbn={} title={:?}", provider_id.as_str(), candidate.isbn, candidate.title);
                let lookup = providers.lookup_isbn_with(&provider_id, &candidate.isbn).await.expect("provider IDs originate from this registry");
                let (status, subjects, detail) = match lookup.result {
                    Err(error) => (SubjectEnrichmentStatus::Failed, Vec::new(), Some(error.to_string())),
                    Ok(lookup) => {
                        let mut matching = lookup.records.into_iter().filter(|record| record.isbns.iter().any(|isbn| lookup.queried_isbns.contains(isbn))).collect::<Vec<_>>();
                        match matching.len() {
                            0 => (SubjectEnrichmentStatus::NoMatch, Vec::new(), None),
                            1 => {
                                let subjects = provider_subjects(provider_id.as_str(), matching.pop().expect("one matching record"))?;
                                if subjects.is_empty() {
                                    (SubjectEnrichmentStatus::NoMatch, subjects, Some("matching record contained no supported subjects or classifications".to_owned()))
                                } else {
                                    (SubjectEnrichmentStatus::Updated, subjects, None)
                                }
                            }
                            count => (SubjectEnrichmentStatus::Ambiguous, Vec::new(), Some(format!("provider returned {count} records for the same ISBN"))),
                        }
                    }
                };
                let imported_subjects = {
                    let mut connection = self.db.get().map_err(BackendError::operation)?;
                    db::apply_external_subjects(&mut connection, candidate.content_hash.clone(), provider_id.as_str(), &candidate.isbn, subjects, status.as_str(), detail.as_deref()).map_err(BackendError::operation)?
                };
                let outcome = SubjectEnrichmentOutcome { content_hash: candidate.content_hash, provider_id: provider_id.as_str().to_owned(), status, imported_subjects, detail };
                eprintln!("SUBJECT_ENRICHMENT result provider={} isbn={} status={:?} imported_subjects={} detail={:?}", outcome.provider_id, candidate.isbn, outcome.status, outcome.imported_subjects, outcome.detail);
                outcomes.push(outcome);
            }
        }
        Ok(outcomes)
    }

    pub async fn enrich_missing_subjects_from_library_of_congress(&self, limit: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentOutcome>, BackendError> {
        let providers = ExternalMetadataProviders::new().with(LibraryOfCongressProvider::new().map_err(BackendError::operation)?);
        self.enrich_missing_subjects(&providers, limit, retry_failed).await
    }
}

fn provider_subjects(provider_id: &str, record: book_metadata::external::ExternalMetadataRecord) -> Result<Vec<PublicationSubject>, BackendError> {
    let mut subjects = Vec::new();
    for heading in record.subjects {
        let name = heading.display_path();
        let authority = match heading.scheme {
            ExternalSubjectScheme::Lcsh => "lcsh".to_owned(),
            ExternalSubjectScheme::Lcgft => "lcgft".to_owned(),
            ExternalSubjectScheme::Other(authority) => authority,
        };
        subjects.push(PublicationSubject::new(heading.authority_uri, name, provider_id, Some(authority), None).map_err(BackendError::operation)?);
    }
    for classification in record.classifications {
        let authority = match classification.scheme {
            ExternalClassificationScheme::Lcc => "lcc".to_owned(),
            ExternalClassificationScheme::Ddc => "ddc".to_owned(),
            ExternalClassificationScheme::Other(authority) => authority,
        };
        subjects.push(PublicationSubject::new(None, &classification.code, provider_id, Some(authority), Some(classification.code.clone())).map_err(BackendError::operation)?);
    }
    subjects.sort_by(|left, right| (left.authority(), left.name(), left.code()).cmp(&(right.authority(), right.name(), right.code())));
    subjects.dedup();
    Ok(subjects)
}
