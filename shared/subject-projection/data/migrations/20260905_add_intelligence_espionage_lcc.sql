PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- LCC Schedule J exact political/government intelligence headings.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:0bd9b806-d6e7-5653-9162-336e3727d6be','lcc','JF1525.I6',0),
('unified:0bd9b806-d6e7-5653-9162-336e3727d6be','lcc','JK468.I6',1);

COMMIT;
