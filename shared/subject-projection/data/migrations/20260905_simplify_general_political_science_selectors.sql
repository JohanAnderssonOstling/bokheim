PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Collapse the imported overlapping JA selectors into a deliberate broad leaf.
DELETE FROM source_selector
WHERE concept_id='unified:68cb5226-6697-5d3e-b247-e0bf9d5deafb';

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:68cb5226-6697-5d3e-b247-e0bf9d5deafb','lcc','JA',0),
('unified:68cb5226-6697-5d3e-b247-e0bf9d5deafb','lcc','JA1..JA92@1',1);

COMMIT;
