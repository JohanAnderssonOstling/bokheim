-- Shorten and reconcile the Economics & Business branch labels and routes.
-- Collapse the redundant Finance wrapper, unify the two economic-theory
-- branches, and place industrial management beneath Management.
-- This migration is intentionally idempotent and follows the Economics and
-- Finance reconciliation migration from the same date.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Reconcile the two Personal Finance concepts before their parents are
-- collapsed, preserving the LCC selector on the richer BISAC concept.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:a4738965-71c5-585a-bb09-395037b009c5', 'lcc', 'HG179..HG179@2', 0);

DELETE FROM concept
WHERE concept_id = 'unified:152efcb9-35f0-53ac-a2c1-33cadd7e6cc8';

-- Promote Finance itself into Economics & Business and move the former
-- wrapper's other children beneath it.
DELETE FROM concept_parent
WHERE parent_concept_id = 'bokheim:subject:economics:finance';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 'unified:103d333e-4f6d-5d1c-9c4b-950677c8d681', 0, 2),
    ('unified:23c07efb-fccb-56b4-88cd-6f8cbdbe3b11', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 1),
    ('unified:fecd1a39-de8f-5c99-b01f-b3f02f8e4992', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 2),
    ('unified:19ca416d-d7e1-58d0-9b49-cb9dae321855', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 3),
    ('unified:c6837963-d028-591d-802f-553cb979fe49', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 4),
    ('unified:45bcabe6-1129-54e8-9852-cb132c3d4892', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 5),
    ('unified:c1329268-e817-5303-ae5e-fbaff3709107', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 6),
    ('unified:8fd0add6-3e26-5d58-9e43-eec09368decc', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 7),
    ('unified:1438360a-b0e5-5744-8634-90f1c188cc60', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 8),
    ('unified:29da64de-ac3b-562e-bec9-9a652d99529a', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 9),
    ('unified:a4738965-71c5-585a-bb09-395037b009c5', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 10),
    ('unified:0217a31b-3b3a-59e6-8d94-2fae2411a7a9', 'unified:66cf212d-88c2-5e3a-9cd3-7b4d76217132', 0, 11);

DELETE FROM concept
WHERE concept_id = 'bokheim:subject:economics:finance';

-- Fold the LCC HB theory tree into the curated theory branch and use one
-- concise visible label.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('bokheim:subject:economics:theory', 'lcc', 'HB', 0),
    ('bokheim:subject:economics:theory', 'lcc', 'HB1..HB3840@1', 1);

DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:2fc32450-d6ba-5776-8668-8a1566dec875';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84', 'bokheim:subject:economics:theory', 0, 16),
    ('unified:a02b8f15-ed32-53a0-8727-ab3832d05bbc', 'bokheim:subject:economics:theory', 0, 17),
    ('unified:208e41a4-dd10-5d11-9b3c-b55a2dd805fb', 'bokheim:subject:economics:theory', 0, 18),
    ('unified:901c3f59-aa41-537c-b2ad-8a6e11cb7ad1', 'bokheim:subject:economics:theory', 0, 19),
    ('unified:b1dbdf6f-d14a-5ac2-903f-8539b1e38d6a', 'bokheim:subject:economics:theory', 0, 20),
    ('unified:f5f4f190-a20f-5326-a6cb-8dba38a409e6', 'bokheim:subject:economics:theory', 0, 21),
    ('unified:9d86501e-f3dd-566c-851c-217d989f32d7', 'bokheim:subject:economics:theory', 0, 22),
    ('unified:f51cc378-eecc-5c86-b4ae-5c0f6536ea25', 'bokheim:subject:economics:theory', 0, 23),
    ('unified:4ad4c662-a6ef-5130-afb4-8ca85f3adc51', 'bokheim:subject:economics:theory', 0, 24),
    ('unified:99af0f9b-f986-51c7-8680-83de2ba28323', 'bokheim:subject:economics:theory', 0, 25),
    ('unified:1e3174a0-64cd-566d-aeeb-8871875f48c5', 'bokheim:subject:economics:theory', 0, 26),
    ('unified:76c6cd35-cbda-5ead-ac6b-dd10258eebf7', 'bokheim:subject:economics:theory', 0, 27);

DELETE FROM concept
WHERE concept_id = 'unified:2fc32450-d6ba-5776-8668-8a1566dec875';

UPDATE concept
SET preferred_label = 'Economic Theory'
WHERE concept_id = 'bokheim:subject:economics:theory';

-- Use the concise Management label and move the industrial-management
-- category out of Industries, Land Use & Labor into that branch.
UPDATE concept
SET preferred_label = 'Management'
WHERE concept_id = 'bokheim:subject:economics:management';

DELETE FROM concept_parent
WHERE concept_id = 'unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b'
  AND parent_concept_id = 'unified:e1e1a593-e41e-5a0d-9243-65711bec3cec';

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:5437a2f2-b2f6-5e63-8f2b-4afe345ea47b', 'bokheim:subject:economics:management', 0, 21);

COMMIT;
