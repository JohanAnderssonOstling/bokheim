PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE ideology_merge(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO ideology_merge VALUES
('unified:2cc6f214-202a-5278-ae34-c4483714c899','bokheim:politics:ideology:anarchism'),
('unified:a02b8f15-ed32-53a0-8727-ab3832d05bbc','bokheim:politics:ideology:capitalism');

WITH candidates AS (
 SELECT m.target_id,s.system_id,s.selector,
        row_number() OVER (PARTITION BY m.target_id,s.system_id ORDER BY s.ordinal,s.selector)-1 AS new_ordinal
 FROM ideology_merge m JOIN source_selector s ON s.concept_id=m.source_id
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,new_ordinal FROM candidates;

DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM ideology_merge);
DROP TABLE ideology_merge;
COMMIT;
