-- PD is Germanic philology and languages, so it belongs to Language rather
-- than the Germanic & Nordic literature group.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM lcc_selector
WHERE concept_id=5587 AND selector='PD';

INSERT OR IGNORE INTO lcc_selector (concept_id,selector)
VALUES (5536,'PD');

COMMIT;
