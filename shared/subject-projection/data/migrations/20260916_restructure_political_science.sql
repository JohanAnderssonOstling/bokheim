-- Align Politics & Government with the LCC distinction between JA (political
-- science) and JC (political theory). Move regional politics out of
-- International Relations because these subjects describe domestic politics.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id, preferred_label)
VALUES (54933, 'By Region');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
VALUES (54933, 5326, 8);

-- Political science (JA) is a sibling of Political Theory (JC).
DELETE FROM concept_parent
WHERE concept_id = 5369 AND parent_concept_id = 5690;
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
VALUES (5369, 5326, 0);

-- Regional politics subjects describe politics within regions/countries.
DELETE FROM concept_parent
WHERE concept_id IN (11786,11787,11788,11789,11790,11791,11792,11793)
  AND parent_concept_id = 5693;
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
SELECT concept_id, 54933,
       ROW_NUMBER() OVER (ORDER BY preferred_label) - 1
FROM concept
WHERE concept_id IN (11786,11787,11788,11789,11790,11791,11792,11793);

COMMIT;
