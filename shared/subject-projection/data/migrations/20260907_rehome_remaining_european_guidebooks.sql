BEGIN;

WITH wanted(label) AS (VALUES ('Montenegrin Travel'),('Balkan Travel'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label=label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label IN
 ('Montenegrin Travel','Balkan Travel');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 2460,v FROM (
 SELECT 'DD258.25' v UNION ALL SELECT 'DD280.5' UNION ALL
 SELECT 'DD801.W627' UNION ALL SELECT 'DD901.M76');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 2493,v FROM (
 SELECT 'DK510.22' v UNION ALL SELECT 'DK549' UNION ALL SELECT 'DK597');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2470,'DL607');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DR1809' FROM concept WHERE preferred_label='Montenegrin Travel';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DR7' FROM concept WHERE preferred_label='Balkan Travel';

COMMIT;
