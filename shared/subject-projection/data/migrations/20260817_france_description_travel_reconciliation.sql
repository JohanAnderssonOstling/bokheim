-- Route French description/travel literature through the existing Travel
-- destination rather than through the visible History hierarchy.

BEGIN IMMEDIATE;

-- Preserve the detailed LCC coverage on the canonical Travel > Europe > France
-- concept, which already carries the BISAC France travel identifier.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
    ('unified:4957d188-138d-5a0c-b218-08f443563cb6','lcc','DC21..DC29.3@2',0);

-- The dated children describe travel accounts from those periods.  They stay
-- together, now directly beneath the France travel destination.
UPDATE concept_parent
SET parent_concept_id='unified:4957d188-138d-5a0c-b218-08f443563cb6'
WHERE parent_concept_id='unified:1a743ace-ac78-5108-a20f-ce2d1a668848';

-- Remove the now-redundant History > France > Description and travel concept
-- explicitly so the migration is safe even when foreign-key cascades are off.
DELETE FROM concept_parent
WHERE concept_id='unified:1a743ace-ac78-5108-a20f-ce2d1a668848'
   OR parent_concept_id='unified:1a743ace-ac78-5108-a20f-ce2d1a668848';
DELETE FROM source_selector WHERE concept_id='unified:1a743ace-ac78-5108-a20f-ce2d1a668848';
DELETE FROM source_label WHERE concept_id='unified:1a743ace-ac78-5108-a20f-ce2d1a668848';
DELETE FROM concept WHERE concept_id='unified:1a743ace-ac78-5108-a20f-ce2d1a668848';

COMMIT;
