-- Finance is already partitioned into ranged leaves. Remove restored umbrella
-- copies from Finance and its nonleaf grouping concepts.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Keep only the exact HG class marker on the Finance navigation parent.
DELETE FROM source_selector
WHERE concept_id=5142 AND system_id='lcc' AND selector<>'HG';

-- These nonleaf concepts are completely represented by their ranged children.
DELETE FROM source_selector
WHERE concept_id IN (5127,5183,5192,5209,5229,5230) AND system_id='lcc';

-- Collapse the redundant nested foreign-exchange ranges on this leaf.
DELETE FROM source_selector WHERE concept_id=5147 AND system_id='lcc';
INSERT INTO source_selector VALUES(5147,'lcc','HG3810..HG4000');

COMMIT;
