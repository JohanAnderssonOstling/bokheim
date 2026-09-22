PRAGMA foreign_keys=ON;
BEGIN;

-- General organizations by region.
INSERT INTO concept(preferred_label)
SELECT 'Organizations' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Organizations');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,4671,0 FROM concept WHERE preferred_label='Organizations';

CREATE TEMP TABLE _ddc_new_child(parent_id INTEGER,label TEXT,selector TEXT PRIMARY KEY);
INSERT INTO _ddc_new_child VALUES
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'North America','061'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'British Isles','062'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'Central Europe','063'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'France & Monaco','064'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'Italy','065'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'Iberia','066'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'Eastern Europe & Russia','067'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Organizations'),'Other Regions','068');

-- Newspapers by region.
INSERT INTO concept(preferred_label)
SELECT 'Newspapers' WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Newspapers');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3768,0 FROM concept WHERE preferred_label='Newspapers';
INSERT INTO _ddc_new_child VALUES
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'North America','071'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'British Isles','072'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'Central Europe','073'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'France & Monaco','074'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'Italy','075'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'Iberia','076'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'Eastern Europe & Russia','077'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'Scandinavia','078'),
 ((SELECT concept_id FROM concept WHERE preferred_label='Newspapers'),'Other Regions','079');

-- General literary collections by original language.
INSERT INTO _ddc_new_child VALUES
 (9042,'American English','081'),
 (9042,'English','082'),
 (9042,'Other Germanic','083'),
 (9042,'French, Occitan & Catalan','084'),
 (9042,'Italian & Romanian','085'),
 (9042,'Spanish & Portuguese','086'),
 (9042,'Slavic','087'),
 (9042,'Scandinavian','088'),
 (9042,'Other Languages','089');

INSERT INTO concept(preferred_label)
SELECT n.label FROM _ddc_new_child n
WHERE NOT EXISTS(
 SELECT 1 FROM concept_parent cp
 JOIN concept child ON child.concept_id=cp.concept_id
 WHERE cp.parent_concept_id=n.parent_id AND child.preferred_label=n.label
);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT child.concept_id,n.parent_id,0
FROM _ddc_new_child n
JOIN concept child ON child.concept_id=(
 SELECT max(candidate.concept_id) FROM concept candidate
 WHERE candidate.preferred_label=n.label
   AND NOT EXISTS(SELECT 1 FROM concept_parent used WHERE used.concept_id=candidate.concept_id)
);

INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
VALUES
 ('060',(SELECT concept_id FROM concept WHERE preferred_label='Organizations')),
 ('070',3768),
 ('080',9042);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT n.selector,child.concept_id
FROM _ddc_new_child n
JOIN concept_parent cp ON cp.parent_concept_id=n.parent_id
JOIN concept child ON child.concept_id=cp.concept_id AND child.preferred_label=n.label;

DROP TABLE _ddc_new_child;
COMMIT;
