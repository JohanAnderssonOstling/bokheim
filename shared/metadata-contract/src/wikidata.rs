//! Source evidence is separate from accepted author identity identifiers.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WikidataBookEvidence {
    pub wikidata_id: String,
    pub work_ids: Vec<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub authors: Vec<WikidataAuthorEvidence>,
    pub classifications: Vec<WikidataClassificationEvidence>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WikidataAuthorEvidence {
    pub wikidata_id: String,
    pub name: Option<String>,
    pub aliases: Vec<String>,
    pub birth_year: Option<i32>,
    pub death_year: Option<i32>,
    pub identifiers: Vec<WikidataAuthorityId>,
    pub image: Option<String>,
    pub source_entity: String,
    /// unlinked, authority, corroborated, provisional, ambiguous, or conflict.
    pub identity_match: String,
    pub open_library_author_id: Option<String>,
    pub shared_work_count: u32,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WikidataAuthorityId {
    pub authority: String,
    pub value: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WikidataClassificationEvidence {
    pub scheme: String,
    pub notation: String,
    pub source_entity: String,
    pub property: String,
    pub inferred: bool,
}
