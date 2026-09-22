-- Correct earlier shortenings that dropped or distorted meaningful scope.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- This LCC caption is a cross-reference rather than a browseable subject.
-- Its range remains covered by the World War II parent selector.
DELETE FROM concept
WHERE concept_id='bokheim:history-lcc-full:ad045430-f508-5326-9f2f-f5f751fcdf71';

WITH rename(concept_id, preferred_label) AS (VALUES
('bokheim:lcc-detail:af74957b-a409-53e6-a5ad-9f3e98c4ba30','Plains, Plateaus & Related Landforms'),
('bokheim:subject-bibliography:system-analysis-systems-engineering-operations-research','Systems Analysis/Engineering & Operations Research'),
('bokheim:lcc-detail:62df43a6-dc08-5e05-b8fc-6f97fcd7bc68','College, Intramural & Interscholastic Athletics'),
('bokheim:history-lcc-full:b9b66893-fc86-564c-acf5-c59acebae224','Cross-National & Cross-Period Events'),
('bokheim:lcc-detail:9052ab96-d4b9-5a51-ad96-e1a73266e714','Poetry & Lyric-Poetry Studies'),
('unified:ff93c4a4-0ccc-5503-a178-2ddb04f202b0','International Associations & Conferences'),
('bokheim:lcc-detail:b6bed5b9-8844-503f-ae4d-237e6ec70df6','Norwegian Literary Theory & Study'),
('bokheim:lcc-detail:2ccf4466-f0d0-59fc-8676-141ce48a6b93','Old Norse Literary Theory & Study'),
('bokheim:history-lcc-full:0ef1dd8a-e25e-553e-afa2-86f28da2354d','Medical Services, Hospitals & Red Cross'),
('bokheim:lcc-detail:d9d5442b-623a-533c-a4dd-3e7555cd5994','Indo-Aryan Regional & Etymological Vocabulary'),
('bokheim:lcc-detail:447bda02-5d02-53dc-9f09-a6e8e2ecf2c6','Primary–Secondary Education Relations'),
('bokheim:lcc-detail:0eba087a-91e8-5d6c-b44c-b8c5ccbc9342','Liquid- & Vapor-Fuel Engines'),
('unified:76c7355e-394e-5864-82d4-b371a81b86fa','Special Subjects, Figures & Religious Art'),
('bokheim:history-lcc-full:c7152445-32a5-5ab4-9b5d-5be085372459','Malta Before 870: Ancient & Byzantine Rule')
)
UPDATE concept
SET preferred_label = (
    SELECT rename.preferred_label
    FROM rename
    WHERE rename.concept_id = concept.concept_id
)
WHERE concept_id IN (SELECT concept_id FROM rename);

COMMIT;
