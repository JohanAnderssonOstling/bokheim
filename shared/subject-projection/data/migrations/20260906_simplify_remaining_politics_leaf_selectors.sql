PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Keep BISAC selectors, replacing redundant LCC details with one leaf range.
DELETE FROM source_selector
WHERE concept_id='unified:d7d2d86b-50ab-506e-8f27-b291d4f7430b'
  AND system_id='lcc';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES('unified:d7d2d86b-50ab-506e-8f27-b291d4f7430b','lcc','K3150..K3370@2',1);

DELETE FROM source_selector
WHERE concept_id='unified:67b62f28-1b6f-5259-98e4-c86e58ad9607'
  AND system_id='lcc';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES('unified:67b62f28-1b6f-5259-98e4-c86e58ad9607','lcc','JF20..JF1177@2',1);

DELETE FROM source_selector
WHERE concept_id='unified:ddb4088f-b10a-53e0-8f09-0351e2cc0a1f'
  AND system_id='lcc';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES('unified:ddb4088f-b10a-53e0-8f09-0351e2cc0a1f','lcc','HX806..HX811@2',1);

COMMIT;
