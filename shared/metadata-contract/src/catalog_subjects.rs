//! Classification-only catalog evidence, independent of an ISBN or OL identity.
use crate::{CatalogSubjectMatch, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, RichWorkMetadataMatch};
pub const METHOD_PREFIX: &str = "library_of_congress_title_author:";

pub fn classification_record(query: &EditionIdentityQuery, matched: &CatalogSubjectMatch) -> Option<RichWorkMetadataMatch> {
    use crate::matching::{author_credits, author_names_compatible, bibliographic_match_key, title_qualifiers};
    if !matches!(matched.provider_id.as_str(), "library_of_congress" | "wikidata") || matched.record_ids.is_empty() || matched.record_ids.len() > 128 || matched.record_ids.iter().any(|id| id.is_empty() || id.len() > 256) { return None; }
    if matched.provider_id=="wikidata" && matched.record_ids.iter().any(|id|!id.strip_prefix('Q').is_some_and(|n|!n.is_empty() && n.bytes().all(|b|b.is_ascii_digit()) && n.parse::<u64>().is_ok_and(|n|n>0))){return None;}
    let full = bibliographic_match_key(&query.title);
    let remote = bibliographic_match_key(&matched.title);
    let main = |s: &str| bibliographic_match_key(s.split(':').next().unwrap_or(s));
    if full.is_empty() || !(full == remote || full == bibliographic_match_key(&matched.main_title) || main(&query.title) == remote) { return None; }
    if title_qualifiers(&query.title) != title_qualifiers(&matched.title) { return None; }
    let authors = author_credits(&query.authors);
    if !authors.iter().any(|local| matched.authors.iter().any(|remote| author_names_compatible(local,remote))) { return None; }
    let mut classifications = matched.classifications.clone();
    classifications.retain(|c| c.scheme == ClassificationScheme::LibraryOfCongress && crate::classification_consensus_notation(c.scheme,&c.notation).is_some());
    if classifications.is_empty() { return None; }
    for c in &mut classifications {
        c.source = ClassificationSource::Work;
        c.evidence = matched.record_ids.iter().map(|id| ClassificationEvidence {
            // The method prefix identifies the lookup, and its suffix preserves
            // the MARC control number even when no ISBN exists.
            method: format!("{}_title_author:{id}",matched.provider_id), isbn13: String::new(), open_library_work_id: String::new(), open_library_edition_id: String::new(),
        }).collect();
    }
    Some(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications, authors: Vec::new(), places: Vec::new(), people: Vec::new(), popularity: None, description: None, subjects: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;
    #[test]
    fn catalog_evidence_round_trips_and_cannot_assign_an_isbn() {
        let q = EditionIdentityQuery { query_id:"1".into(),title:"French history".into(),authors:vec!["Jane Smith".into()],publishers:vec![],publication_year:None };
        let m = CatalogSubjectMatch { main_title:q.title.clone(), provider_id:"library_of_congress".into(),record_ids:vec!["12345".into()],title:"French history; a survey".into(),authors:vec!["Smith, Jane".into()],classifications:vec![Classification { scheme:ClassificationScheme::LibraryOfCongress,notation:"DC161".into(),source:ClassificationSource::Work,evidence:vec![] }] };
        let response = EditionIdentityResponse { snapshot:SnapshotDescription {dump_date:"2016".into(),imported_at_ms:1},results:vec![EditionIdentityResult {query_id:"1".into(),status:EditionIdentityStatus::NoMatch,matched:None,candidates:vec![],candidates_truncated:false,catalog_subjects:Some(m)}] };
        let decoded = decode_v2_edition_identity_response(&encode_v2_edition_identity_response(&response).unwrap(),MAX_RESPONSE_BYTES).unwrap();
        let m = decoded.results[0].catalog_subjects.as_ref().unwrap();
        assert_eq!(m.record_ids,vec!["12345"]);
        let r = classification_record(&q,m).unwrap();
        assert!(r.exact_edition_ids.is_empty() && r.open_library_work_id.is_none());
        assert!(r.classifications[0].evidence[0].isbn13.is_empty());
        assert_eq!(r.classifications[0].evidence[0].method,"library_of_congress_title_author:12345");
        let mut wrong=q.clone();wrong.authors=vec!["John Smith".into()];
        assert!(classification_record(&wrong,m).is_none());
    }
}
