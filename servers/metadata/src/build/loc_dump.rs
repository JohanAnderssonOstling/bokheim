//! ISBN/LCC evidence imported into the existing metadata snapshot from LC MARC.

use crate::Isbn13;
use crate::{error, Classification, ClassificationScheme, ClassificationSource, Connection, MetadataError, WorkClassificationMatch};
use std::collections::BTreeSet;

pub(crate) fn lookup_lcc(connection: &Connection, isbn13: Isbn13) -> Result<BTreeSet<String>, MetadataError> {
    let mut statement = connection.prepare_cached("SELECT DISTINCT notation FROM isbn_lcc WHERE isbn13=?1 ORDER BY notation").map_err(error)?;
    let rows = statement.query_map([isbn13], |row| row.get::<_, String>(0)).map_err(error)?;
    rows.collect::<Result<BTreeSet<_>, _>>().map_err(error)
}

pub(crate) fn merge_lcc(matches: &mut Vec<WorkClassificationMatch>, codes: BTreeSet<String>) {
    // Do not attach one ISBN's evidence to conflicting Open Library works.
    if matches.len() > 1 {
        return;
    }
    let codes = codes.into_iter().filter_map(|code| subject_projection::canonical_lcc_notation(&code)).collect::<BTreeSet<_>>();
    if codes.is_empty() {
        return;
    }
    if matches.is_empty() {
        // An authority record is not an Open Library edition: never manufacture OL IDs.
        matches.push(WorkClassificationMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications: Vec::new() });
    }
    let classifications = &mut matches[0].classifications;
    for notation in codes {
        if let Some(existing) = classifications.iter_mut().find(|c| c.scheme == ClassificationScheme::LibraryOfCongress && c.notation == notation) {
            existing.source = ClassificationSource::ExactEdition;
        } else {
            classifications.push(Classification { evidence: Vec::new(), scheme: ClassificationScheme::LibraryOfCongress, notation, source: ClassificationSource::ExactEdition });
        }
    }
    classifications.sort();
}

/// Fill the rich fields already supported by the wire contract. Bibliographic
/// fields remain in lc_record for a future provider-neutral identity response.
pub(crate) fn enrich(connection: &Connection, results: &mut [crate::RichIsbnMetadataResult]) -> Result<(), MetadataError> {
    let mut query = connection.prepare_cached("SELECT r.metadata FROM lc_record_isbn i JOIN lc_record r USING(lc_record_id) WHERE i.isbn13=?1 ORDER BY i.lc_record_id").map_err(error)?;
    #[derive(serde::Deserialize)]
    struct Record {
        #[serde(default)]
        descriptions: Vec<String>,
        #[serde(default)]
        subjects: Vec<crate::SubjectHeading>,
    }
    for result in results {
        if result.matches.len() > 1 {
            continue;
        }
        let Some(isbn) = Isbn13::parse(&result.requested_isbn) else { continue };
        let mut descriptions = BTreeSet::new();
        let mut subjects = BTreeSet::new();
        let rows = query.query_map([isbn], |r| r.get::<_, String>(0)).map_err(error)?;
        for row in rows {
            let record: Record = serde_json::from_str(&row.map_err(error)?).map_err(error)?;
            descriptions.extend(record.descriptions.into_iter().filter(|s| !s.trim().is_empty() && s.len() <= crate::MAX_DESCRIPTION_BYTES));
            subjects.extend(record.subjects);
        }
        if descriptions.is_empty() && subjects.is_empty() {
            continue;
        }
        if result.matches.is_empty() {
            result.matches.push(crate::RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications: Vec::new(), authors: Vec::new(), description: None, subjects: Vec::new() });
            result.status = crate::LookupStatus::Matched;
        }
        let matched = &mut result.matches[0];
        if matched.description.as_ref().is_none_or(|s| s.trim().is_empty()) && descriptions.len() == 1 {
            matched.description = descriptions.into_iter().next();
        }
        if matched.subjects.is_empty() {
            matched.subjects = subjects.into_iter().collect();
        }
    }
    Ok(())
}
