-- Move Archaeology from Sociology to the Social Sciences root.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
UPDATE concept_parent SET parent_concept_id='unified:f8f652e6-6244-5dc5-97b7-9dece2de275f',display_order=104 WHERE concept_id='unified:b591dbe3-ecfc-5dc2-8906-73c5938b6659' AND parent_concept_id='unified:1dc4999e-4875-54c4-be21-e8b23a259be2';
COMMIT;
