-- Retain only the 50 states and the regional grouping nodes needed to reach
-- them beneath Regions of the United States. Full detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE us_state_name(name TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO us_state_name VALUES
 ('Alabama'),('Alaska'),('Arizona'),('Arkansas'),('California'),('Colorado'),
 ('Connecticut'),('Delaware'),('Florida'),('Georgia'),('Hawaii'),('Idaho'),
 ('Illinois'),('Indiana'),('Iowa'),('Kansas'),('Kentucky'),('Louisiana'),
 ('Maine'),('Maryland'),('Massachusetts'),('Michigan'),('Minnesota'),
 ('Mississippi'),('Missouri'),('Montana'),('Nebraska'),('Nevada'),
 ('New Hampshire'),('New Jersey'),('New Mexico'),('New York'),
 ('North Carolina'),('North Dakota'),('Ohio'),('Oklahoma'),('Oregon'),
 ('Pennsylvania'),('Rhode Island'),('South Carolina'),('South Dakota'),
 ('Tennessee'),('Texas'),('Utah'),('Vermont'),('Virginia'),('Washington'),
 ('West Virginia'),('Wisconsin'),('Wyoming');

CREATE TEMP TABLE us_region_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
 VALUES ('unified:62708214-9d20-5e1f-b7ef-d3e4d591bab9')
 UNION
 SELECT child.concept_id
 FROM tree parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO us_region_tree SELECT concept_id FROM tree;

CREATE TEMP TABLE us_region_keep(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE keep(concept_id) AS (
 SELECT tree.concept_id
 FROM us_region_tree tree
 JOIN concept c ON c.concept_id=tree.concept_id
 JOIN us_state_name state ON state.name=c.preferred_label
 UNION
 SELECT parent.parent_concept_id
 FROM keep child
 JOIN concept_parent parent ON parent.concept_id=child.concept_id
 WHERE parent.parent_concept_id IN (SELECT concept_id FROM us_region_tree)
)
INSERT INTO us_region_keep SELECT concept_id FROM keep;

CREATE TEMP TABLE us_region_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO us_region_remove
SELECT concept_id FROM us_region_tree
WHERE concept_id NOT IN (SELECT concept_id FROM us_region_keep);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_region_remove)
   OR parent_concept_id IN (SELECT concept_id FROM us_region_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM us_region_remove);

DROP TABLE us_region_remove;
DROP TABLE us_region_keep;
DROP TABLE us_region_tree;
DROP TABLE us_state_name;
COMMIT;
