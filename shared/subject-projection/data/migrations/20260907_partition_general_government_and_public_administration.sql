BEGIN;

-- A literal broad catalogue notation remains on the parent, but must not act
-- as permissive coverage for every more specific JF number.
DELETE FROM lcc_range
WHERE concept_id=5691 AND class_letters='JF'
  AND start_value='20' AND end_value='2112';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5691,'JF20-2112');

WITH wanted(label) AS (VALUES
 ('Executive Government'),
 ('Legislatures & Parliaments'),
 ('Referendums & Direct Democracy'),
 ('Public Administration'),
 ('Civil Service & Public Employment'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5691,0 FROM concept
WHERE preferred_label IN (
 'Executive Government','Legislatures & Parliaments',
 'Referendums & Direct Democracy','Public Administration',
 'Civil Service & Public Employment');

-- Reuse generic political subjects already present elsewhere in the
-- polyhierarchy instead of creating duplicates.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
 (11756,5691,1), -- Political Rights & Participation
 (11782,5691,1), -- Campaigns & Elections
 (5336,5691,1),  -- Corruption & Misconduct
 (11785,5691,1); -- Political Parties

-- Comparative government is the official JF20-JF1197.2 division. Preserve
-- the literal broad notation directly on that leaf.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('JF','20','1197.2',5334);
DELETE FROM lcc_range
WHERE concept_id=5334 AND class_letters='JF'
  AND start_value='20' AND end_value='1177';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5334,'JF20-1197.2');

-- More specific official divisions override the broader comparative span.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'JF','251','289',concept_id FROM concept
WHERE preferred_label='Executive Government';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'JF','501','619',concept_id FROM concept
WHERE preferred_label='Legislatures & Parliaments';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'JF','491','497',concept_id FROM concept
WHERE preferred_label='Referendums & Direct Democracy';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('JF','799','1197.2',11756);
-- Civic Participation already owns this official division in the curated
-- taxonomy; remove its obsolete truncated duplicate.
DELETE FROM lcc_range
WHERE concept_id=5329 AND class_letters='JF'
  AND start_value='799' AND end_value='1177';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('JF','1001','1048.52',11782);
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('JF','1081','1083',5336);

-- Public administration retains its literal broad notation, while its
-- numeric span and the more specific civil-service and party spans resolve
-- to leaves.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'JF','1315.2','2009.99',concept_id FROM concept
WHERE preferred_label='Public Administration';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'JF1315.2-2112' FROM concept
WHERE preferred_label='Public Administration';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'JF','1411','1678',concept_id FROM concept
WHERE preferred_label='Civil Service & Public Employment';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES ('JF','2011','2112',11785);

COMMIT;
