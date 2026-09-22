PRAGMA foreign_keys=ON;
BEGIN;

CREATE TEMP TABLE _bible_group(label TEXT PRIMARY KEY,parent_id INTEGER,ddc TEXT UNIQUE);
INSERT INTO _bible_group VALUES
 ('Gospels & Acts',12373,'226'),
 ('Epistles',12373,'227'),
 ('Apocrypha & Pseudepigrapha',5729,'229'),
 ('Creeds & Catechisms',4581,'238');

INSERT INTO concept(preferred_label)
SELECT g.label FROM _bible_group g
WHERE NOT EXISTS(
 SELECT 1 FROM concept_parent cp JOIN concept c ON c.concept_id=cp.concept_id
 WHERE cp.parent_concept_id=g.parent_id AND c.preferred_label=g.label
);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,g.parent_id,0 FROM _bible_group g JOIN concept c ON c.concept_id=(
 SELECT max(c2.concept_id) FROM concept c2 WHERE c2.preferred_label=g.label
 AND NOT EXISTS(SELECT 1 FROM concept_parent cp WHERE cp.concept_id=c2.concept_id)
);

CREATE TEMP TABLE _group_id(label TEXT PRIMARY KEY,concept_id INTEGER);
INSERT INTO _group_id
SELECT g.label,c.concept_id FROM _bible_group g
JOIN concept_parent cp ON cp.parent_concept_id=g.parent_id
JOIN concept c ON c.concept_id=cp.concept_id AND c.preferred_label=g.label;

-- Give specific Bible parts their new grouping parent.
DELETE FROM concept_parent WHERE concept_id IN(12564,12565,12566,12567,12569,12660);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (12564,(SELECT concept_id FROM _group_id WHERE label='Gospels & Acts'),0),
 (12565,(SELECT concept_id FROM _group_id WHERE label='Gospels & Acts'),0),
 (12566,(SELECT concept_id FROM _group_id WHERE label='Epistles'),0),
 (12567,(SELECT concept_id FROM _group_id WHERE label='Epistles'),0),
 (12569,(SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha'),0),
 (12660,(SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha'),0);
-- Retain the Catholic route while also exposing it beneath general creeds.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (12909,(SELECT concept_id FROM _group_id WHERE label='Creeds & Catechisms'),1);

INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT g.ddc,i.concept_id FROM _bible_group g JOIN _group_id i USING(label);

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BS2549','BS2625.6',(SELECT concept_id FROM _group_id WHERE label='Gospels & Acts')),
 ('BS2640','BS2815.6',(SELECT concept_id FROM _group_id WHERE label='Epistles')),
 ('BS1691','BS1830',(SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha')),
 ('BS2831','BS2970',(SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha')),
 ('BT990','BT1010',(SELECT concept_id FROM _group_id WHERE label='Creeds & Catechisms')),
 ('BT1029','BT1040',(SELECT concept_id FROM _group_id WHERE label='Creeds & Catechisms'));

INSERT OR IGNORE INTO bisac_selector(concept_id,selector) VALUES
 ((SELECT concept_id FROM _group_id WHERE label='Gospels & Acts'),'REL006800'),
 ((SELECT concept_id FROM _group_id WHERE label='Gospels & Acts'),'REL006710'),
 ((SELECT concept_id FROM _group_id WHERE label='Epistles'),'REL006810'),
 ((SELECT concept_id FROM _group_id WHERE label='Epistles'),'REL006820'),
 ((SELECT concept_id FROM _group_id WHERE label='Epistles'),'REL006720'),
 ((SELECT concept_id FROM _group_id WHERE label='Epistles'),'REL006860'),
 ((SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha'),'REL006790'),
 ((SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha'),'REL006880'),
 ((SELECT concept_id FROM _group_id WHERE label='Apocrypha & Pseudepigrapha'),'REL006890'),
 ((SELECT concept_id FROM _group_id WHERE label='Creeds & Catechisms'),'REL009000');

DROP TABLE _group_id;
DROP TABLE _bible_group;
COMMIT;
