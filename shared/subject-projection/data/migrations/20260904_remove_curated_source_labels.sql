-- Historical source captions are maintenance provenance and are omitted from
-- the immutable curated client taxonomy. Keep the empty table for runtime and
-- schema compatibility. Preferred labels and external code selectors remain.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
DELETE FROM source_label;
COMMIT;
