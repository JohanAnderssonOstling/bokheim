-- Keep Criminology directly under Social Sciences. The intermediate
-- Social Issues & Services route is too ambiguous and duplicates the subject
-- in navigation because Criminology already has a direct Social Sciences edge.
-- This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

DELETE FROM concept_parent
WHERE concept_id = 'unified:eef99aec-2e4e-552d-89e2-5908191745c9'
  AND parent_concept_id = 'unified:3cf48471-040f-5902-98c1-b29effbc984e';

UPDATE concept_parent
SET ordinal = 0
WHERE concept_id = 'unified:eef99aec-2e4e-552d-89e2-5908191745c9'
  AND parent_concept_id = 'unified:f8f652e6-6244-5dc5-97b7-9dece2de275f';

COMMIT;
