-- Keep U.S. states grouped under broad regions, but make every state a leaf in
-- the curated taxonomy. Full state detail remains in the master taxonomy.

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

CREATE TEMP TABLE us_state(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE us_subtree(concept_id) AS (
 VALUES ('unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a')
 UNION
 SELECT child.concept_id
 FROM concept_parent child
 JOIN us_subtree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO us_state
SELECT item.concept_id
FROM us_subtree item
JOIN concept c ON c.concept_id=item.concept_id
JOIN us_state_name state_name ON state_name.name=c.preferred_label;

CREATE TEMP TABLE us_state_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id
 FROM us_state state
 JOIN concept_parent child ON child.parent_concept_id=state.concept_id
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO us_state_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_state_detail)
   OR parent_concept_id IN (SELECT concept_id FROM us_state_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM us_state_detail);

DROP TABLE us_state_detail;
DROP TABLE us_state;
DROP TABLE us_state_name;
COMMIT;
