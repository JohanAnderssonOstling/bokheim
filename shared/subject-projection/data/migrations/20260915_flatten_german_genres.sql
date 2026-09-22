-- Flatten German-specific genres from the shared European genre branches.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Drama' WHERE concept_id=3888;
UPDATE concept SET preferred_label='Poetry' WHERE concept_id=4435;

DELETE FROM concept_parent
WHERE (concept_id=3888 AND parent_concept_id=3885)
   OR (concept_id=4435 AND parent_concept_id=4432);

INSERT OR IGNORE INTO concept_parent (concept_id, parent_concept_id, ordinal)
SELECT 3888, 4244, COALESCE(MAX(ordinal), -1) + 1
FROM concept_parent WHERE parent_concept_id=4244;

INSERT OR IGNORE INTO concept_parent (concept_id, parent_concept_id, ordinal)
SELECT 4435, 4244, COALESCE(MAX(ordinal), -1) + 1
FROM concept_parent WHERE parent_concept_id=4244;

INSERT OR IGNORE INTO lcc_selector (concept_id, selector) VALUES
  (4435, 'PT500-PT597'),
  (3888, 'PT605-PT709');

COMMIT;
