PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Keep BISAC selectors on the non-leaf. Replace only the imported, overlapping
-- LCC selectors with residual general/early-US and Confederate-era ranges.
DELETE FROM source_selector
WHERE concept_id='unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f'
  AND system_id='lcc';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f','lcc','JK',32),
('unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f','lcc','JK1..JK400@1',33),
('unified:ea222eb6-ef35-58d6-a407-6455fc1fe95f','lcc','JK9660..JK9993@1',34);

-- Each child is already a deliberately broad leaf; nested imported ranges are
-- redundant and can interfere with deterministic ownership.
DELETE FROM source_selector WHERE concept_id IN (
 'bokheim:politics:us:executive',
 'bokheim:politics:us:congress',
 'bokheim:politics:us:participation',
 'bokheim:politics:us:parties',
 'bokheim:politics:us:states'
);
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:politics:us:executive','lcc','JK501..JK899@2',0),
('bokheim:politics:us:congress','lcc','JK900..JK1439@2',0),
('bokheim:politics:us:participation','lcc','JK1717..JK2249@2',0),
('bokheim:politics:us:parties','lcc','JK2250..JK2399@2',0),
('bokheim:politics:us:states','lcc','JK2400..JK9599@2',0);

COMMIT;
