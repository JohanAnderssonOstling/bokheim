PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- JS is wholly Local government / Municipal government. This curated leaf is
-- intentionally broad, so imported country and city subranges are redundant.
DELETE FROM source_selector
WHERE concept_id='unified:8d7f7376-5c6f-543e-b881-37f0ff458a8a';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:8d7f7376-5c6f-543e-b881-37f0ff458a8a','lcc','JS',0),
('unified:8d7f7376-5c6f-543e-b881-37f0ff458a8a','lcc','JS39..JS8500@1',1);

COMMIT;
