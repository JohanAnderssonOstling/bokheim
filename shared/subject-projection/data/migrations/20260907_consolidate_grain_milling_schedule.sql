-- The sole historical subdivision is below the expansion threshold. Keep the
-- official grain-milling schedule as a single leaf.

DELETE FROM concept_parent
WHERE parent_concept_id = 351
  AND concept_id = 9579;

DELETE FROM lcc_range
WHERE concept_id IN (351, 9579);

DELETE FROM concept
WHERE concept_id = 9579
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

INSERT INTO lcc_range(start_code, end_code, concept_id)
VALUES ('TS2120', 'TS2159', 351);
