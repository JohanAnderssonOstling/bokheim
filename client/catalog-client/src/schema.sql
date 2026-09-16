CREATE TABLE IF NOT EXISTS catalog_client_schema_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    schema_version INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS catalog_subject_publication_cache (
    epoch TEXT NOT NULL,
    entry_id INTEGER NOT NULL,
    publication_payload BLOB NOT NULL,
    PRIMARY KEY(epoch,entry_id)
) STRICT;
