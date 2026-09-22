-- Treat the complete DDC 962 section as Egypt in the curated taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:60bcae6e-928b-57c6-8c80-e2cf9d7a6246'
  AND system_id = 'ddc'
  AND selector = '962';

INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
VALUES ('unified:b1da6c49-e4e6-5dca-b82a-8f37a1793e80', 'ddc', '962', 0);

COMMIT;
