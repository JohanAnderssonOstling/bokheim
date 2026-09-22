BEGIN;

-- Broad general-history material belongs directly on its contextual parent;
-- do not create a redundant "General Histories" navigation leaf.
UPDATE lcc_range SET concept_id=3408 WHERE concept_id=51087;
UPDATE lcc_range SET concept_id=13699 WHERE concept_id=51137;

DELETE FROM concept WHERE concept_id IN (51087,51137);

COMMIT;
