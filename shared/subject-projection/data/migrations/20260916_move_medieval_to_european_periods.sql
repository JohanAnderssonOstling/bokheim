-- Place the general European Medieval period beside the other European
-- periods instead of under the cross-regional Eras grouping.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'By Period'
WHERE concept_id = 11498 AND preferred_label = 'By Era';

UPDATE concept_parent
SET parent_concept_id = 11498
WHERE concept_id = 3608 AND parent_concept_id = 3618;

COMMIT;
