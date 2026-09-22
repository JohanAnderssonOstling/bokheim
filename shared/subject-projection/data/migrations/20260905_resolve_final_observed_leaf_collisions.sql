-- Resolve the final observed exact-code collisions using the official LCC
-- captions as the authoritative owner.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND (
 (concept_id='bokheim:history-lcc:ad35eb64-2b14-5f7f-8b99-68ab15e087c9' AND selector='DB83')
 OR
 (concept_id='bokheim:history-lcc-full:101e50e2-0a81-5c37-8e54-a5c562e6de92' AND selector='DT107.826')
);

COMMIT;
