-- Separate the mixed surgery/emergency umbrella and merge duplicate source concepts.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- Preserve the DDC mapping from the redundant generic surgery concept.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:a9891854-d470-5937-89cc-7fbb7df25452',system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='unified:a9891854-d470-5937-89cc-7fbb7df25452'
                   AND existing.system_id=source.system_id),0)
FROM source_selector source
WHERE source.concept_id='unified:07ee3601-3985-5ef5-bd85-6b732f53a0bb'
  AND NOT EXISTS (
    SELECT 1 FROM source_selector existing
    WHERE existing.concept_id='unified:a9891854-d470-5937-89cc-7fbb7df25452'
      AND existing.system_id=source.system_id
      AND existing.selector=source.selector
  );

-- The detailed LCC anesthesiology concept is retained; add its BISAC coverage.
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:d6585b91-ec20-514a-aaad-0e2c245a9c8b',system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id='unified:d6585b91-ec20-514a-aaad-0e2c245a9c8b'
                   AND existing.system_id=source.system_id),0)
FROM source_selector source
WHERE source.concept_id='unified:d5bca6c3-d28d-59a2-bed9-2eef5ec42949'
  AND NOT EXISTS (
    SELECT 1 FROM source_selector existing
    WHERE existing.concept_id='unified:d6585b91-ec20-514a-aaad-0e2c245a9c8b'
      AND existing.system_id=source.system_id
      AND existing.selector=source.selector
  );

-- Keep both original source labels for matching and diagnostics.
INSERT INTO source_label(concept_id,label,ordinal)
SELECT 'unified:a9891854-d470-5937-89cc-7fbb7df25452',label,
       COALESCE((SELECT max(existing.ordinal)+1 FROM source_label existing
                 WHERE existing.concept_id='unified:a9891854-d470-5937-89cc-7fbb7df25452'),0)
FROM source_label source
WHERE source.concept_id='unified:07ee3601-3985-5ef5-bd85-6b732f53a0bb'
  AND NOT EXISTS (SELECT 1 FROM source_label existing
                  WHERE existing.concept_id='unified:a9891854-d470-5937-89cc-7fbb7df25452'
                    AND existing.label=source.label);

-- Make room for Surgery immediately before Emergency & Critical Care.
UPDATE concept_parent
SET display_order=display_order+1
WHERE parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18'
  AND display_order>=5;

UPDATE concept
SET preferred_label='Emergency & Critical Care'
WHERE concept_id='bokheim:subject:health:surgery-emergency';

UPDATE concept_parent
SET parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18',
    display_order=4
WHERE concept_id='unified:a9891854-d470-5937-89cc-7fbb7df25452'
  AND parent_concept_id='bokheim:subject:health:surgery-emergency';

UPDATE concept_parent
SET display_order=5
WHERE concept_id='bokheim:subject:health:surgery-emergency'
  AND parent_concept_id='unified:353c91d5-53e5-56a8-9cd6-544a69566d18';

-- Remove the duplicate top-level Anesthesiology and generic Surgery concepts.
DELETE FROM concept_parent
WHERE concept_id IN (
  'unified:d5bca6c3-d28d-59a2-bed9-2eef5ec42949',
  'unified:07ee3601-3985-5ef5-bd85-6b732f53a0bb'
);
DELETE FROM concept
WHERE concept_id IN (
  'unified:d5bca6c3-d28d-59a2-bed9-2eef5ec42949',
  'unified:07ee3601-3985-5ef5-bd85-6b732f53a0bb'
);

COMMIT;
