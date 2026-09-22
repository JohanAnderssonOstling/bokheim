PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- This is the deliberately pruned fallback leaf for subclass JC. The imported
-- selector set redundantly repeated exact numbers and overlapping subranges.
-- Keep only the bare subclass code and one permissive, low-priority range;
-- narrower curated selectors resolve first by their higher priority.
DELETE FROM source_selector
WHERE concept_id='unified:45517dc7-6533-59f8-b204-566d1acf319a';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:45517dc7-6533-59f8-b204-566d1acf319a','lcc','JC',0),
('unified:45517dc7-6533-59f8-b204-566d1acf319a','lcc','JC11..JC607@1',1);

COMMIT;
