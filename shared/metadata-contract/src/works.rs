//! Work identity lookup does not assert an edition or ISBN.
use serde::{Deserialize, Serialize};

pub const MAX_WORK_QUERIES: usize = 128;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkLookupRequest {
    pub queries: Vec<WorkIdentityQuery>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkIdentityQuery {
    pub query_id: String,
    pub title: String,
    pub authors: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkLookupResponse {
    pub results: Vec<WorkIdentityResult>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkIdentityStatus {
    Matched,
    Ambiguous,
    NoMatch,
    InsufficientMetadata,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkIdentityResult {
    pub query_id: String,
    pub status: WorkIdentityStatus,
    pub matched: Option<WorkIdentityMatch>,
    pub candidates: Vec<WorkIdentityMatch>,
    pub candidates_truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkIdentityMatch {
    pub open_library_work_id: String,
    pub title: String,
    pub authors: Vec<String>,
}
