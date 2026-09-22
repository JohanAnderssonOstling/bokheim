PRAGMA foreign_keys=ON;
BEGIN;

CREATE TEMP TABLE _ddc_encyclopedia_group(label TEXT PRIMARY KEY, selector TEXT UNIQUE);
INSERT INTO _ddc_encyclopedia_group VALUES
 ('Germanic','033'),
 ('French, Occitan & Catalan','034'),
 ('Italian & Romanian','035'),
 ('Iberian','036'),
 ('Other Languages','039');

INSERT INTO concept(preferred_label)
SELECT label FROM _ddc_encyclopedia_group g
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=g.label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,4705,0
FROM _ddc_encyclopedia_group g JOIN concept c ON c.preferred_label=g.label;

-- Move existing language leaves beneath their corresponding DDC groups.
DELETE FROM concept_parent
WHERE parent_concept_id=4705 AND concept_id IN (7804,9723,9211,9795,9094,6493,8614,9705,9402);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child_id,group_id,0 FROM (
 SELECT 7804 child_id,(SELECT concept_id FROM concept WHERE preferred_label='Germanic') group_id
 UNION ALL SELECT 9723,(SELECT concept_id FROM concept WHERE preferred_label='Germanic')
 UNION ALL SELECT 9211,(SELECT concept_id FROM concept WHERE preferred_label='French, Occitan & Catalan')
 UNION ALL SELECT 9795,(SELECT concept_id FROM concept WHERE preferred_label='Iberian')
 UNION ALL SELECT 9094,(SELECT concept_id FROM concept WHERE preferred_label='Other Languages')
 UNION ALL SELECT 6493,(SELECT concept_id FROM concept WHERE preferred_label='Other Languages')
 UNION ALL SELECT 8614,(SELECT concept_id FROM concept WHERE preferred_label='Other Languages')
 UNION ALL SELECT 9705,(SELECT concept_id FROM concept WHERE preferred_label='Other Languages')
 UNION ALL SELECT 9402,(SELECT concept_id FROM concept WHERE preferred_label='Other Languages')
);

INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('030',4704),('031',9429),('032',9429),('037',7303),('038',7319);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,c.concept_id FROM _ddc_encyclopedia_group g JOIN concept c ON c.preferred_label=g.label;

DROP TABLE _ddc_encyclopedia_group;
COMMIT;
