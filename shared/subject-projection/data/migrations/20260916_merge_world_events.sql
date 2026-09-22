-- World Events is a duplicate general World-history node.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT 3701,'World History Events',COALESCE(MAX(ordinal)+1,0)
FROM source_label WHERE concept_id=3701;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 3701,selector FROM lcc_selector WHERE concept_id=10630;
DELETE FROM lcc_selector WHERE concept_id=10630;
DELETE FROM bisac_selector WHERE concept_id=10630;
DELETE FROM source_label WHERE concept_id=10630;
DELETE FROM concept_parent WHERE concept_id=10630;
DELETE FROM concept WHERE concept_id=10630;

INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT 3701,'Multinational History',COALESCE(MAX(ordinal)+1,0)
FROM source_label WHERE concept_id=3701;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 3701,selector FROM lcc_selector WHERE concept_id=10634;
DELETE FROM lcc_selector WHERE concept_id=10634;
DELETE FROM bisac_selector WHERE concept_id=10634;
DELETE FROM source_label WHERE concept_id=10634;
DELETE FROM concept_parent WHERE concept_id=10634;
DELETE FROM concept WHERE concept_id=10634;

COMMIT;
