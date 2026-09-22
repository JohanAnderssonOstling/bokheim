-- Use short names aligned with the LCC period hierarchy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

WITH renames(concept_id, old_label, new_label) AS (VALUES
    (3667, 'Early Modern Era, 1453–1648', 'Early Modern'),
    (3668, '17th Century, 1601–1715', '17th Century'),
    (3669, '18th Century, 1715–1789', '18th Century'),
    (3670, 'Long 19th C., 1789–1914/1920', '19th Century')
)
INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
SELECT r.concept_id, r.old_label, 0
FROM renames r
JOIN concept c ON c.concept_id = r.concept_id
WHERE c.preferred_label = r.old_label
  AND NOT EXISTS (
      SELECT 1 FROM source_label s
      WHERE s.concept_id = r.concept_id AND s.label = r.old_label
  );

UPDATE concept
SET preferred_label = CASE concept_id
    WHEN 3667 THEN 'Early Modern'
    WHEN 3668 THEN '17th Century'
    WHEN 3669 THEN '18th Century'
    WHEN 3670 THEN '19th Century'
END
WHERE concept_id IN (3667, 3668, 3669, 3670);

COMMIT;
