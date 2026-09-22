-- Split the combined root into Sports and Recreation. Remove the Games &
-- Activities and Recreation. Leisure wrappers by promoting their children.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Recreation'
WHERE concept_id = 'unified:8caf059f-7a4f-5d99-a59d-bc38f1a72355';

-- Reuse the curated sports grouping as the new top-level Sports root.
UPDATE concept
SET preferred_label = 'Sports'
WHERE concept_id = 'unified:be682165-416b-5f95-8119-674ad1555e74';

DELETE FROM concept_parent
WHERE concept_id = 'unified:be682165-416b-5f95-8119-674ad1555e74'
  AND parent_concept_id = 'unified:8caf059f-7a4f-5d99-a59d-bc38f1a72355';

CREATE TEMP TABLE recreation_wrapper(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO recreation_wrapper VALUES
('unified:c8f5d33b-d707-58c0-a019-1b7d3d170e05'),
('unified:b7d58c32-97a7-53fc-a001-e2cc3bfe6588');

-- Promote games and recreation subjects directly into Recreation. The legacy
-- Sports branch is handled separately below.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT route.concept_id,
       'unified:8caf059f-7a4f-5d99-a59d-bc38f1a72355',
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = route.concept_id), 0),
       route.display_order
FROM concept_parent route
WHERE route.parent_concept_id IN (SELECT concept_id FROM recreation_wrapper)
  AND route.concept_id <> 'unified:69bcbce0-9e01-5ebd-85cd-bab2a0a8e658'
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id = route.concept_id
        AND existing.parent_concept_id = 'unified:8caf059f-7a4f-5d99-a59d-bc38f1a72355'
  );

-- Move the older direct sport groupings from Recreation into Sports.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT route.concept_id,
       'unified:be682165-416b-5f95-8119-674ad1555e74',
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = route.concept_id), 0),
       route.display_order
FROM concept_parent route
WHERE route.parent_concept_id = 'unified:8caf059f-7a4f-5d99-a59d-bc38f1a72355'
  AND route.concept_id IN (
      'unified:6b6a0d79-0e15-5146-aa72-f592a8036c2c',
      'unified:c581a994-ecaf-52fd-943b-74f2ba7ea09c',
      'unified:88e7c149-4f22-5f3b-929c-76aae70ea377',
      'unified:8b91ae1c-b4cc-5c35-8d27-cb88ac18efd0'
  )
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id = route.concept_id
        AND existing.parent_concept_id = 'unified:be682165-416b-5f95-8119-674ad1555e74'
  );

DELETE FROM concept_parent
WHERE parent_concept_id = 'unified:8caf059f-7a4f-5d99-a59d-bc38f1a72355'
  AND concept_id IN (
      'unified:6b6a0d79-0e15-5146-aa72-f592a8036c2c',
      'unified:c581a994-ecaf-52fd-943b-74f2ba7ea09c',
      'unified:88e7c149-4f22-5f3b-929c-76aae70ea377',
      'unified:8b91ae1c-b4cc-5c35-8d27-cb88ac18efd0'
  );

-- Absorb the older Sports node's source metadata and children into Sports.
CREATE TEMP TABLE legacy_sports(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO legacy_sports VALUES ('unified:69bcbce0-9e01-5ebd-85cd-bab2a0a8e658');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT route.concept_id,
       'unified:be682165-416b-5f95-8119-674ad1555e74',
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = route.concept_id), 0),
       route.display_order
FROM concept_parent route
WHERE route.parent_concept_id IN (SELECT concept_id FROM legacy_sports)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id = route.concept_id
        AND existing.parent_concept_id = 'unified:be682165-416b-5f95-8119-674ad1555e74'
  );

-- Preserve the old generic Sports selector and caption on the new root.
INSERT INTO source_label(concept_id,label,ordinal)
SELECT 'unified:be682165-416b-5f95-8119-674ad1555e74',label.label,
       COALESCE((SELECT max(x.ordinal)+1 FROM source_label x
                 WHERE x.concept_id='unified:be682165-416b-5f95-8119-674ad1555e74'),0)
       + row_number() OVER (ORDER BY label.label) - 1
FROM source_label label
WHERE label.concept_id IN (SELECT concept_id FROM legacy_sports)
  AND NOT EXISTS (SELECT 1 FROM source_label x
                  WHERE x.concept_id='unified:be682165-416b-5f95-8119-674ad1555e74'
                    AND x.label=label.label);
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:be682165-416b-5f95-8119-674ad1555e74',s.system_id,s.selector,
       COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x
                 WHERE x.concept_id='unified:be682165-416b-5f95-8119-674ad1555e74'
                   AND x.system_id=s.system_id),0)
       + row_number() OVER (PARTITION BY s.system_id ORDER BY s.selector) - 1
FROM source_selector s
WHERE s.concept_id IN (SELECT concept_id FROM legacy_sports)
  AND NOT EXISTS (SELECT 1 FROM source_selector x
                  WHERE x.concept_id='unified:be682165-416b-5f95-8119-674ad1555e74'
                    AND x.system_id=s.system_id AND x.selector=s.selector);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM recreation_wrapper UNION SELECT concept_id FROM legacy_sports)
   OR parent_concept_id IN (SELECT concept_id FROM recreation_wrapper UNION SELECT concept_id FROM legacy_sports);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM recreation_wrapper UNION SELECT concept_id FROM legacy_sports)
  AND NOT EXISTS (SELECT 1 FROM concept_parent p
                  WHERE p.concept_id=concept.concept_id OR p.parent_concept_id=concept.concept_id);

DROP TABLE legacy_sports;
DROP TABLE recreation_wrapper;
COMMIT;
