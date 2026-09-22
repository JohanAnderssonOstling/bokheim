-- Auxiliary Sciences of History is a grouping node; its children own the
-- specific classifications.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM lcc_selector WHERE concept_id=3514;
DELETE FROM bisac_selector WHERE concept_id=3514;

COMMIT;
