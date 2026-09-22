-- Replace invalid cross-jurisdiction template ranges with precise selectors
-- from the general comparative-law and space-law schedules.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Move precise ranges out of overly broad retained parents.
DELETE FROM source_selector WHERE system_id='lcc' AND selector IN (
 'K3941..K3974@4',
 'K5575..K5582@4',
 'K600..K615@2',
 'K623..K968@2',
 'K3280..K3282@4'
);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('unified:7979eca7-9715-5cb9-94c5-f0bffd6c9f84','lcc','K3941..K3974@4',0),
 ('unified:1f397bb9-9db9-5478-9b3e-789c8ffb8a14','lcc','K5575..K5582@4',0),
 ('unified:5e71375e-ded3-577c-a4fa-d6e9ee673b21','lcc','K600..K615@2',0),
 ('unified:5e71375e-ded3-577c-a4fa-d6e9ee673b21','lcc','K623..K968@2',1),
 ('unified:4b2cae00-8ac3-5e97-90ee-ee6b4d0892bf','lcc','K3280..K3282@4',0);

UPDATE concept SET preferred_label='International Trade Law'
WHERE concept_id='unified:7979eca7-9715-5cb9-94c5-f0bffd6c9f84';
UPDATE concept SET preferred_label='Private & Civil Law'
WHERE concept_id='unified:5e71375e-ded3-577c-a4fa-d6e9ee673b21';
UPDATE concept SET preferred_label='Church–State Relations'
WHERE concept_id='unified:4b2cae00-8ac3-5e97-90ee-ee6b4d0892bf';

-- Merge the detailed space-law concept into the concise curated leaf.
UPDATE source_selector
SET concept_id='unified:9966051a-3ede-5c7f-9b0b-97b6c7cfd3d5'
WHERE concept_id='unified:7c44493a-3b50-57eb-92c6-b638212c7c4b';
DELETE FROM concept
WHERE concept_id='unified:7c44493a-3b50-57eb-92c6-b638212c7c4b';
UPDATE concept SET preferred_label='Space Law'
WHERE concept_id='unified:9966051a-3ede-5c7f-9b0b-97b6c7cfd3d5';

-- The source schedule has an exact interpretation range, but no equally
-- precise general range for the previous reform/criticism umbrella.
UPDATE concept SET preferred_label='Legal Interpretation'
WHERE concept_id='bokheim:subject:law:systems:reform';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:subject:law:systems:reform','lcc','K288..K296@4',0);

COMMIT;
