-- Attach the 20 individually verified missing codes from the BISG History list.
-- Global Indigenous themes must not be confined to the Americas branch.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE additions(code TEXT PRIMARY KEY,id INTEGER,label TEXT,parent INTEGER);
INSERT INTO additions(code,id) VALUES
 ('HIS006000',3400),('HIS067000',3603),('HIS026050',3501),
 ('HIS032010',12631),('HIS032020',12633),('HIS032030',11342),
 ('HIS069000',3398),('HIS070000',3398);

CREATE TEMP TABLE indigenous_root(id INTEGER PRIMARY KEY);
INSERT INTO indigenous_root
SELECT c.concept_id FROM concept c JOIN concept_parent cp USING(concept_id)
WHERE cp.parent_concept_id=3616 AND c.preferred_label='Indigenous Peoples';
INSERT INTO concept(preferred_label)
SELECT 'Indigenous Peoples' WHERE NOT EXISTS(SELECT 1 FROM indigenous_root);
INSERT INTO indigenous_root SELECT last_insert_rowid()
WHERE NOT EXISTS(SELECT 1 FROM indigenous_root);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id,3616,0 FROM indigenous_root;

INSERT INTO additions(code,label,parent) VALUES
 ('HIS051000','Expeditions & Discoveries',3319),
 ('HIS052000','Historical Geography',3319),
 ('HIS068000','Hispanic & Latino',3616),
 ('HIS071000','Asian American & Pacific Islander',3616),
 ('HIS072000','Disability',3616),
 ('HIS073000','Historical & Collective Memory',3319);
INSERT INTO additions(code,label,parent)
SELECT code,label,r.id FROM (
 SELECT 'HIS028010' AS code,'Creation & Origins' AS label
 UNION ALL SELECT 'HIS028020','Migration'
 UNION ALL SELECT 'HIS028030','Archaeological Stages & Oral History'
 UNION ALL SELECT 'HIS028040','Contact, Invasion & Exploration'
 UNION ALL SELECT 'HIS028050','Colonial Interactions'
 UNION ALL SELECT 'HIS028060','Modern'
) CROSS JOIN indigenous_root r;

-- Reuse matching children on a repeat run; allocate every new ID centrally.
UPDATE additions SET id=(SELECT c.concept_id FROM concept c
 JOIN concept_parent cp USING(concept_id)
 WHERE c.preferred_label=additions.label AND cp.parent_concept_id=additions.parent)
WHERE id IS NULL;
INSERT INTO concept(preferred_label) SELECT label FROM additions WHERE id IS NULL;
UPDATE additions SET id=(SELECT max(concept_id) FROM concept WHERE preferred_label=additions.label)
WHERE id IS NULL;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id,parent,0 FROM additions WHERE parent IS NOT NULL;

CREATE TEMP TABLE assignment_guard(ok INTEGER CHECK(ok=1));
INSERT INTO assignment_guard
SELECT NOT EXISTS(SELECT 1 FROM additions a JOIN bisac_selector b ON b.selector=a.code
 WHERE b.concept_id<>a.id);
INSERT OR IGNORE INTO bisac_selector(concept_id,selector) SELECT id,code FROM additions;
DROP TABLE assignment_guard;
DROP TABLE additions;
DROP TABLE indigenous_root;
COMMIT;
