-- DDC is intentionally excluded from the client-curated taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector WHERE system_id='ddc';
COMMIT;
