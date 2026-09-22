-- Separate domestic/leisure subjects from personal development and use short,
-- browse-friendly labels. Source captions and selectors remain attached to the
-- existing concepts.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept
SET preferred_label = 'Home & Hobbies'
WHERE concept_id = 'unified:9f69631a-7c1d-5df8-bb57-ce270aa0c118';

INSERT OR IGNORE INTO concept(concept_id, preferred_label)
VALUES ('bokheim:subject:personal-growth', 'Personal Growth');
INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
VALUES ('bokheim:subject:personal-growth', 'Personal Growth', 0);

CREATE TEMP TABLE self_help_context(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO self_help_context
SELECT child.concept_id
FROM concept_parent route
JOIN concept child ON child.concept_id = route.concept_id
WHERE route.parent_concept_id = 'unified:9f69631a-7c1d-5df8-bb57-ce270aa0c118'
  AND child.preferred_label = 'Self-Help';

-- Promote the existing subjects directly into the new top-level root.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT route.concept_id,
       'bokheim:subject:personal-growth',
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = route.concept_id), 0),
       route.display_order
FROM concept_parent route
WHERE route.parent_concept_id IN (SELECT concept_id FROM self_help_context)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id = route.concept_id
        AND existing.parent_concept_id = 'bokheim:subject:personal-growth'
  );

UPDATE concept SET preferred_label = 'Addiction & Recovery'
WHERE concept_id = 'bokheim:self-help:addiction-recovery';
UPDATE concept SET preferred_label = 'Motivation & Self-Management'
WHERE concept_id = 'bokheim:self-help:growth-management';
UPDATE concept SET preferred_label = 'Mindfulness & Inner Practice'
WHERE concept_id = 'bokheim:self-help:inner-practices';
UPDATE concept SET preferred_label = 'Relationships & Identity'
WHERE concept_id = 'bokheim:self-help:relationships-identity';
UPDATE concept SET preferred_label = 'Creativity & Expression'
WHERE concept_id = 'bokheim:self-help:lifestyle-creativity';
UPDATE concept SET preferred_label = 'Grief, Safety & Life Changes'
WHERE concept_id = 'bokheim:self-help:safety-transitions';

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM self_help_context)
   OR parent_concept_id IN (SELECT concept_id FROM self_help_context);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM self_help_context)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id = concept.concept_id
         OR remaining.parent_concept_id = concept.concept_id
  );

DROP TABLE self_help_context;
COMMIT;
