-- The two existing subdivisions are below the expansion threshold. Keep the
-- official electric-lighting schedule as one leaf and eliminate nested ranges.

DELETE FROM concept_parent
WHERE parent_concept_id = 256
  AND concept_id IN (6407, 6768);

DELETE FROM lcc_selector
WHERE concept_id IN (6407, 6768);

DELETE FROM concept
WHERE concept_id IN (6407, 6768)
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
WHERE concept_id = 256;

INSERT INTO lcc_range(start_code, end_code, concept_id)
VALUES ('TK4125', 'TK4399', 256);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (256, 'TK4175'),
  (256, 'TK4188');
