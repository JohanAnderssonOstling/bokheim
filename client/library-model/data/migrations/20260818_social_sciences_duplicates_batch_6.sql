-- Apply the sixth reviewed Social Sciences batch and reconcile the duplicate
-- adult Careers wrappers. Audience-specific juvenile Careers concepts remain
-- separate. This migration is intentionally idempotent.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Keep law-specific concepts distinct with explicit labels.
UPDATE concept
SET preferred_label = 'Cultural Policy Law'
WHERE concept_id = 'bokheim:lcc-detail:59b1a639-0ee4-5c23-92b0-746d67c8e281';

UPDATE concept
SET preferred_label = 'Democracy Law'
WHERE concept_id = 'bokheim:lcc-detail:aa603956-1147-506a-98bc-ddff5d7c5c71';

-- Diplomacy: retain the detailed LCC concept, add BISAC coverage, and place it
-- directly beneath the curated International Relations branch.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:465a0f44-4380-591f-bd81-fd9ceec060ee', 'bisac', 'POL011010', 0);

DELETE FROM concept
WHERE concept_id = 'unified:6b56b209-6ec0-5554-a0df-728b329cbe22';

UPDATE concept_parent
SET parent_concept_id = 'bokheim:politics:international-global'
WHERE concept_id = 'unified:465a0f44-4380-591f-bd81-fd9ceec060ee'
  AND parent_concept_id = 'unified:5e71ac50-3e6f-5e04-bc7e-722c06cf630c';

-- Job Hunting: retain the curated Careers entry with BISAC and LCC coverage.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:e4323cb7-0d86-5ffe-b20e-7d754f7a6a11', 'lcc', 'HF5382.7..HF5382.75@5', 0);

INSERT OR IGNORE INTO source_label(concept_id, label, ordinal) VALUES
    ('unified:e4323cb7-0d86-5ffe-b20e-7d754f7a6a11', 'Job hunting', 0);

DELETE FROM concept
WHERE concept_id = 'bokheim:business:careers:job-hunting';

-- Merge the remaining Careers & Employment children into the adult Careers
-- branch. Existing equivalent edges win if a future source adds one.
DELETE FROM concept_parent
WHERE parent_concept_id = 'bokheim:business:careers-employment'
  AND EXISTS (
      SELECT 1
      FROM concept_parent existing
      WHERE existing.concept_id = concept_parent.concept_id
        AND existing.parent_concept_id = 'unified:ecfd77fa-ee5f-523c-8310-85742e6d49ec'
  );

UPDATE concept_parent
SET parent_concept_id = 'unified:ecfd77fa-ee5f-523c-8310-85742e6d49ec'
WHERE parent_concept_id = 'bokheim:business:careers-employment';

DELETE FROM concept
WHERE concept_id = 'bokheim:business:careers-employment';

-- Psychoanalysis: retain the detailed clinical concept and expose that same
-- concept under Psychology > Movements.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal) VALUES
    ('unified:95a73359-78ee-5370-bf25-3bbafb4c2dea', 'bisac', 'PSY026000', 0);

INSERT OR IGNORE INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order) VALUES
    ('unified:95a73359-78ee-5370-bf25-3bbafb4c2dea', 'unified:0d5e376a-043f-5516-b265-feff5b23ad2c', 1, 0);

DELETE FROM concept
WHERE concept_id = 'unified:8ed5df44-3fa4-566a-890b-4adefb6c4de0';

COMMIT;
