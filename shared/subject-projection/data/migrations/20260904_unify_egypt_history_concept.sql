-- Use one Egypt history concept beneath both North Africa and the Middle East.
-- The former DDC 962 concept also covered Sudan and South Sudan, so retain that
-- broad selector on North Africa rather than narrowing it incorrectly to Egypt.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
VALUES ('unified:60bcae6e-928b-57c6-8c80-e2cf9d7a6246', 'ddc', '962', 0);

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
VALUES (
    'unified:b1da6c49-e4e6-5dca-b82a-8f37a1793e80',
    'unified:60bcae6e-928b-57c6-8c80-e2cf9d7a6246',
    1,
    0
);

DELETE FROM concept_parent
WHERE concept_id = 'unified:27436e40-1248-56c8-85cf-625069316966';
DELETE FROM concept
WHERE concept_id = 'unified:27436e40-1248-56c8-85cf-625069316966';

COMMIT;
