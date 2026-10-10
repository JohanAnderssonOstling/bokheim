//! File contracts between the builder, server and crawler.
pub(crate) const ANSWER_VERSION: i64 = 1;
pub(crate) const ANSWERS: &str = "
CREATE TABLE snapshot(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL,dump_date TEXT NOT NULL,built_at_ms INTEGER NOT NULL,provider_results_through_ms INTEGER NOT NULL);
CREATE TABLE isbn_answer(isbn13 INTEGER PRIMARY KEY,answer BLOB NOT NULL) WITHOUT ROWID;
CREATE TABLE work_description(work_id INTEGER PRIMARY KEY,text TEXT NOT NULL);
CREATE TABLE edition_candidate(edition_id INTEGER PRIMARY KEY,isbn13 INTEGER,work_id INTEGER,title TEXT NOT NULL,subtitle TEXT,work_title TEXT,authors TEXT NOT NULL,publishers TEXT NOT NULL,book_year INTEGER,classifications BLOB NOT NULL);
CREATE TABLE edition_title_key(key TEXT NOT NULL,edition_id INTEGER NOT NULL,edition_match INTEGER NOT NULL,work_match INTEGER NOT NULL,PRIMARY KEY(key,edition_id)) WITHOUT ROWID;
CREATE TABLE authority_title(key TEXT NOT NULL,author_keys TEXT NOT NULL,source TEXT NOT NULL,control_number TEXT NOT NULL,codes BLOB NOT NULL,PRIMARY KEY(key,source,control_number)) WITHOUT ROWID;
CREATE TABLE wanted(isbn13 INTEGER PRIMARY KEY,rank INTEGER NOT NULL,work_id INTEGER);
CREATE INDEX wanted_by_rank ON wanted(rank,isbn13);
CREATE INDEX wanted_by_work ON wanted(work_id,isbn13);
";
pub(crate) const PROVIDERS: &str = "
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS provider_result(provider TEXT NOT NULL CHECK(provider IN ('loc','librarything')),isbn13 INTEGER NOT NULL,fetched_at_ms INTEGER NOT NULL,status TEXT NOT NULL CHECK(status IN ('matched','no_match','ambiguous')),classifications BLOB NOT NULL,subjects BLOB NOT NULL,PRIMARY KEY(provider,isbn13)) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS provider_result_by_isbn ON provider_result(isbn13,provider);
CREATE TABLE IF NOT EXISTS provider_retry(provider TEXT NOT NULL,isbn13 INTEGER NOT NULL,retry_at_ms INTEGER NOT NULL,error TEXT NOT NULL,PRIMARY KEY(provider,isbn13));
CREATE TABLE IF NOT EXISTS provider_quota(provider TEXT NOT NULL,day INTEGER NOT NULL,used INTEGER NOT NULL,PRIMARY KEY(provider,day));
CREATE TABLE IF NOT EXISTS provider_rate(provider TEXT PRIMARY KEY,next_ms INTEGER NOT NULL);
";
pub(crate) const DEMAND: &str = "
PRAGMA journal_mode=WAL;
CREATE TABLE IF NOT EXISTS demand(isbn13 INTEGER PRIMARY KEY,first_seen_ms INTEGER NOT NULL,last_seen_ms INTEGER NOT NULL,requests INTEGER NOT NULL);
";
pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis().try_into().unwrap_or(i64::MAX)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredAnswer {
    pub result: metadata_contract::RichIsbnMetadataResult,
    pub description_works: Vec<Option<i64>>,
}
