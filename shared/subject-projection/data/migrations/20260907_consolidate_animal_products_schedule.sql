-- All meat and poultry subdivisions are below the expansion threshold. Keep
-- the official animal-products schedule as one broad leaf.

DELETE FROM concept_parent
WHERE parent_concept_id = 350
  AND concept_id IN (7443, 7728, 9616, 9637);

DELETE FROM lcc_range
WHERE concept_id IN (350, 7443, 9637);

DELETE FROM lcc_selector
WHERE concept_id IN (7443, 7728, 9616);

DELETE FROM concept
WHERE concept_id IN (7443, 7728, 9616, 9637)
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
VALUES ('TS1950', 'TS1982', 350);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (350, 'TS1959'),
  (350, 'TS1975'),
  (350, 'TS1968');
