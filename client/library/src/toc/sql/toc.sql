-- Navigation documents derived from publication files.

-- name: upsert_book_toc!
-- param: content_hash: &str
-- param: entry_count: i64
-- param: toc_source: &str
-- param: toc_json: &str
INSERT INTO book_toc(content_hash,entry_count,toc_source,toc_json)
VALUES(:content_hash,:entry_count,:toc_source,:toc_json)
ON CONFLICT(content_hash) DO UPDATE SET
    entry_count=excluded.entry_count,
    toc_source=excluded.toc_source,
    toc_json=excluded.toc_json;

-- name: upsert_book_reading_entry!
-- param: content_hash: &str
-- param: target: &str
INSERT INTO book_reading_entry(content_hash,target)
VALUES(:content_hash,:target)
ON CONFLICT(content_hash) DO UPDATE SET target=excluded.target;
