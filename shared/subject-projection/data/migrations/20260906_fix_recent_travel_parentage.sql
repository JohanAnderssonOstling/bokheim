-- Correct recent national travel selectors that were accidentally attached to
-- Literature > By Region > Europe (4182) instead of the travel hierarchy.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Existing travel leaves.
INSERT OR IGNORE INTO source_selector VALUES(2460,'lcc','DD16');
INSERT OR IGNORE INTO source_selector VALUES(2460,'lcc','DD21.5..DD43');
INSERT OR IGNORE INTO source_selector VALUES(2488,'lcc','DR421..DR429.4');
DELETE FROM source_selector WHERE concept_id=4231 AND system_id='lcc';

-- Remove the duplicate Germany travel node and reparent the newly created leaves.
DELETE FROM concept WHERE concept_id=13898;
UPDATE concept_parent SET parent_concept_id=2455
WHERE concept_id IN (13893,13910,13915,13919) AND parent_concept_id=4182;

COMMIT;
