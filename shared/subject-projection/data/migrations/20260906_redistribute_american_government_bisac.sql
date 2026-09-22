PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id,preferred_label)
VALUES('bokheim:politics:us:judiciary','Judiciary');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
VALUES('bokheim:politics:us:judiciary','unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f',0,3);

UPDATE source_selector
SET concept_id='bokheim:politics:us:congress', ordinal=1
WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f'
  AND system_id='bisac' AND selector='POL006000';
UPDATE source_selector
SET concept_id='bokheim:politics:us:executive', ordinal=1
WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f'
  AND system_id='bisac' AND selector='POL040010';
UPDATE source_selector
SET concept_id='bokheim:politics:us:states', ordinal=1
WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f'
  AND system_id='bisac' AND selector='POL020000';
UPDATE source_selector
SET concept_id='bokheim:politics:us:states', ordinal=2
WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f'
  AND system_id='bisac' AND selector='POL040040';
UPDATE source_selector
SET concept_id='bokheim:politics:us:judiciary', ordinal=0
WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f'
  AND system_id='bisac' AND selector='POL040030';

COMMIT;
