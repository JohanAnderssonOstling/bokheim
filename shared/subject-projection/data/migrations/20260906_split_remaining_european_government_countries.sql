PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE country_split(
 concept_id TEXT PRIMARY KEY,
 label TEXT NOT NULL,
 selector TEXT NOT NULL UNIQUE,
 display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO country_split VALUES
('bokheim:politics:europe:slovakia','Slovakia','JN2240',610),
('bokheim:politics:europe:estonia','Estonia','JN6615',620),
('bokheim:politics:europe:lithuania','Lithuania','JN6745',630);

DELETE FROM source_selector
WHERE concept_id='unified:8f2c2533-1a08-56ff-a786-52e091c4a82b'
  AND system_id='lcc'
  AND selector IN ('JN2240','JN6615','JN6745');

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,label FROM country_split;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'unified:8f2c2533-1a08-56ff-a786-52e091c4a82b',0,display_order
FROM country_split;
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'lcc',selector,0 FROM country_split;

COMMIT;
