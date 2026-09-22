-- Packaging subdivisions are all below the expansion threshold. Consolidate
-- their selectors into a single official schedule leaf.

DELETE FROM concept_parent
WHERE parent_concept_id = 354
  AND concept_id IN (7499, 8744, 9746);

DELETE FROM lcc_range
WHERE concept_id IN (354, 7499, 9746);

DELETE FROM lcc_selector
WHERE concept_id = 8744;

DELETE FROM concept
WHERE concept_id IN (7499, 8744, 9746)
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

INSERT INTO lcc_range(start_code, end_code, concept_id)
VALUES ('TS195', 'TS198.8', 354);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (354, 'TS195.4');
