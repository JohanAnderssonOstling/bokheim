-- Reuse the existing computing Languages branch as Programming Languages
-- beneath Computer Science, preserving its children, codes, and concept ID.
-- Computer Science keeps exact general selectors; its descendants retain
-- ownership of their specific programming-language codes.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Programming Languages'
WHERE concept_id = 'unified:a1f43498-7169-52bb-aa41-697293885e9e';

UPDATE concept_parent
SET parent_concept_id = 'unified:fc1f8eb8-d687-5ea4-969b-2efb0c169828',
    display_order = 10
WHERE concept_id = 'unified:a1f43498-7169-52bb-aa41-697293885e9e'
  AND parent_concept_id = 'bokheim:subject:computers:programming';

COMMIT;
