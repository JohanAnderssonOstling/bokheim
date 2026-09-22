-- Italic is a valid historical family, but it is an unnecessary wrapper in
-- the curated taxonomy when its retained content is only Latin and Romance.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept_parent
SET parent_concept_id = 'unified:a5bf4300-c76d-4c09-86aa-2eeee54db800'
WHERE parent_concept_id = 'unified:201fe7db-e61f-583d-a626-abdf4d58d37a';

-- DDC 470 denotes Latin and the closely related Italic languages. Latin is
-- the closest retained curated concept after removing the family wrapper.
INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
VALUES ('unified:b17623d3-cbfd-5a12-8f5d-97aa904f3563', 'ddc', '470', 0);

DELETE FROM concept_parent
WHERE concept_id = 'unified:201fe7db-e61f-583d-a626-abdf4d58d37a';
DELETE FROM concept
WHERE concept_id = 'unified:201fe7db-e61f-583d-a626-abdf4d58d37a';

COMMIT;
