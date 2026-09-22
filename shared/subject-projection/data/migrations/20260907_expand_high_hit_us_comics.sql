BEGIN;
UPDATE concept SET preferred_label='U.S. Comics by Form & Creator'
WHERE concept_id=12470 AND preferred_label='U.S. Comics';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (12470,'PN6725-6728');
DELETE FROM lcc_range WHERE concept_id=12470;

CREATE TEMP TABLE _comics(label TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _comics VALUES
 ('U.S. Comics — General & History','PN6725','PN6725'),
 ('U.S. Comic Collections','PN6726','PN6726'),
 ('U.S. Comics Creators','PN6727','PN6727'),
 ('U.S. Comic Titles & Characters','PN6728','PN6728');
INSERT INTO concept(preferred_label) SELECT label FROM _comics s
 WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12470,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _comics s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=12470);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _comics s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _comics;
COMMIT;
