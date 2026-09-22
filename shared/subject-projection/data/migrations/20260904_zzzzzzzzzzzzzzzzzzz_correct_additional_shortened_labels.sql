-- Correct additional shortenings that lost meaningful source-label scope.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH rename(concept_id, preferred_label) AS (VALUES
('bokheim:lcc-detail:224699df-c316-5214-8a88-3874010ec76c','School, Intramural & Interscholastic Athletics'),
('bokheim:lcc-detail:0240b750-8c51-57c4-851c-74fedf293b46','Miscellaneous Recipes & Appetizers'),
('bokheim:lcc-detail:59e8d46f-88f0-5faf-bd88-c4001b2b3d29','Color Prints & Metal-Engraved Prints'),
('unified:ee05c235-5e24-5dc1-8701-0017e0785434','Greeting Cards, Postcards, Invitations & Jackets'),
('bokheim:lcc-detail:5feab946-976d-5e24-97a4-fb5223a6f9a7','General Urban Plan Collections'),
('bokheim:history-lcc-full:c76b2d8a-2873-5418-a922-08f0016ce9fb','Background & Effects of American Discovery'),
('unified:3a31472c-d865-5887-b20b-44f3c3db84ad','Neuroscience & Neuropsychiatric Fields'),
('bokheim:lcc-detail:020d0b73-0322-5c2a-a874-9333eae17066','Literature’s Relations to History & Culture'),
('bokheim:lcc-detail:35186021-1122-56db-9db6-716b88d4deba','Cupping, Acupuncture & Counterirritation')
)
UPDATE concept
SET preferred_label = (
    SELECT rename.preferred_label
    FROM rename
    WHERE rename.concept_id = concept.concept_id
)
WHERE concept_id IN (SELECT concept_id FROM rename);

COMMIT;
