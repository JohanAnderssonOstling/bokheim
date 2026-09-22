-- Tobacco and wood-technology subdivisions are all below the expansion
-- threshold. Consolidate both official schedules into broad leaves.

DELETE FROM concept_parent
WHERE (parent_concept_id = 359 AND concept_id IN (7383, 6811, 6435, 8206))
   OR (parent_concept_id = 360 AND concept_id IN (8307, 10036, 9287, 8633, 9865, 7038, 8624));

DELETE FROM lcc_range
WHERE concept_id IN (359, 360, 8206, 8633);

DELETE FROM lcc_selector
WHERE concept_id IN (7383, 6811, 6435, 8307, 10036, 9287, 9865, 7038, 8624);

DELETE FROM concept
WHERE concept_id IN (7383, 6811, 6435, 8206, 8307, 10036, 9287, 8633, 9865, 7038, 8624)
  AND NOT EXISTS (
    SELECT 1 FROM concept_parent p
    WHERE p.concept_id = concept.concept_id
       OR p.parent_concept_id = concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM bisac_selector b WHERE b.concept_id = concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM lcc_selector s WHERE s.concept_id = concept.concept_id
  )
  AND NOT EXISTS (
    SELECT 1 FROM lcc_range r WHERE r.concept_id = concept.concept_id
  );

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TS2220', 'TS2283', 359),
  ('TS800',  'TS915',  360);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (359, 'TS2260'),
  (359, 'TS2270'),
  (359, 'TS2280'),
  (360, 'TS850'),
  (360, 'TS851'),
  (360, 'TS870'),
  (360, 'TS875'),
  (360, 'TS890'),
  (360, 'TS910');
