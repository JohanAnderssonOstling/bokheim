-- Low-use subdivisions do not justify navigational leaves. Consolidate them
-- into the official broad subjects, which can then own their complete ranges.

DELETE FROM concept_parent
WHERE (concept_id = 8189 AND parent_concept_id = 291)
   OR (concept_id IN (6329, 6789) AND parent_concept_id = 294);

DELETE FROM lcc_selector
WHERE concept_id IN (8189, 6329, 6789);

DELETE FROM concept
WHERE concept_id IN (8189, 6329, 6789)
  AND NOT EXISTS (
    SELECT 1 FROM concept_parent p
    WHERE p.concept_id = concept.concept_id
       OR p.parent_concept_id = concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM bisac_selector b WHERE b.concept_id = concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM lcc_range r WHERE r.concept_id = concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM lcc_selector s WHERE s.concept_id = concept.concept_id
  );

DELETE FROM lcc_range
WHERE concept_id = 294
  AND start_code = 'TA803'
  AND end_code = 'TA804';

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TA800', 'TA820.2', 294);

-- Retain the formerly explicit subclasses as exact inputs on the consolidated
-- leaves, including the official printed interval for tunnel history.
INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (291, 'TA2030'),
  (294, 'TA803-804'),
  (294, 'TA807'),
  (294, 'TA815');
