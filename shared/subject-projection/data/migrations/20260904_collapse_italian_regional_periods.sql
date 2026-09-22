-- Italian regional subdivisions retain only broad chronological buckets.
-- Those buckets are leaves in the curated taxonomy; detailed chronology and
-- local subjects remain available in master-taxonomy-v2.sqlite3.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Restore the two Venice buckets removed by the preceding flattening migration.
INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES
('bokheim:history-lcc-full:4d39c93c-c252-51ee-9d79-2ad3572d60fc', 'Early and medieval'),
('bokheim:history-lcc-full:25e93f47-ff2f-59c9-92e9-2c64e0f286c1', 'Modern');
INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
('bokheim:history-lcc-full:4d39c93c-c252-51ee-9d79-2ad3572d60fc', 'Early and medieval', 0),
('bokheim:history-lcc-full:25e93f47-ff2f-59c9-92e9-2c64e0f286c1', 'Modern', 0);
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
('bokheim:history-lcc-full:4d39c93c-c252-51ee-9d79-2ad3572d60fc', 'lcc', 'DG677..DG677.99@3', 0),
('bokheim:history-lcc-full:25e93f47-ff2f-59c9-92e9-2c64e0f286c1', 'lcc', 'DG678..DG678.8@3', 0);

CREATE TEMP TABLE italian_region(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO italian_region
SELECT child.concept_id
FROM concept_parent route
JOIN concept child ON child.concept_id = route.concept_id
JOIN concept italy ON italy.concept_id = route.parent_concept_id
WHERE italy.preferred_label = 'Italy'
  AND child.preferred_label IN (
      'Genoa',
      'Milan. Lombardy',
      'Naples. Kingdom of the Two Sicilies',
      'Papal States (States of the Church). Holy See. Vatican City',
      'Piedmont. Savoy',
      'Rome (Modern city)',
      'Sicily',
      'Tuscany. Florence',
      'Venice'
  );

-- Reattach the restored broad buckets to Venice.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id, venice.concept_id, 0,
       CASE period.preferred_label WHEN 'Early and medieval' THEN 90 ELSE 100 END
FROM concept period
CROSS JOIN concept venice
WHERE period.concept_id IN (
    'bokheim:history-lcc-full:4d39c93c-c252-51ee-9d79-2ad3572d60fc',
    'bokheim:history-lcc-full:25e93f47-ff2f-59c9-92e9-2c64e0f286c1'
)
  AND venice.concept_id IN (SELECT concept_id FROM italian_region)
  AND venice.preferred_label = 'Venice'
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id = period.concept_id
        AND existing.parent_concept_id = venice.concept_id
  );

CREATE TEMP TABLE retained_period(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO retained_period
SELECT child.concept_id
FROM concept_parent route
JOIN concept child ON child.concept_id = route.concept_id
WHERE route.parent_concept_id IN (SELECT concept_id FROM italian_region)
  AND lower(child.preferred_label) IN (
      'early and medieval',
      'early and medieval to 1409',
      'medieval',
      'modern'
  );

CREATE TEMP TABLE regional_detail_to_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE unwanted(concept_id) AS (
    -- Remove every regional child except the retained broad period buckets.
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM italian_region)
      AND child.concept_id NOT IN (SELECT concept_id FROM retained_period)
    UNION
    -- Retained buckets become leaves, so remove all of their descendants.
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM retained_period)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN unwanted parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO regional_detail_to_remove SELECT concept_id FROM unwanted;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM regional_detail_to_remove)
   OR parent_concept_id IN (SELECT concept_id FROM regional_detail_to_remove);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM regional_detail_to_remove);

DROP TABLE regional_detail_to_remove;
DROP TABLE retained_period;
DROP TABLE italian_region;
COMMIT;
