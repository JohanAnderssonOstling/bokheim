-- Move clear subject-biography entries into the Biography branch.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label=CASE concept_id
  WHEN 9153 THEN 'Museum Personnel'
  WHEN 7083 THEN 'Arts Patrons'
  WHEN 6922 THEN 'Art Teachers'
  WHEN 4715 THEN 'Bibliographers'
  WHEN 7218 THEN 'Copyists'
  WHEN 6281 THEN 'Book Collectors'
  WHEN 8610 THEN 'Librarians'
  WHEN 7939 THEN 'Crystallographers'
  WHEN 7822 THEN 'Reformers'
  ELSE preferred_label END
WHERE concept_id IN (9153,7083,6922,4715,7218,6281,8610,7939,7822);

DELETE FROM concept_parent
WHERE concept_id IN (9153,7083,6922,4715,7218,6281,8610,7939,7822);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3212,ROW_NUMBER() OVER (ORDER BY preferred_label)-1
FROM concept
WHERE concept_id IN (9153,7083,6922,4715,7218,6281,8610,7939,7822);

COMMIT;
