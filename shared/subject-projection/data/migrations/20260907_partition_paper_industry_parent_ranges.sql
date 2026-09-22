-- Move residual general works and manufacturer catalogs off the nonleaf paper
-- parent without creating low-use topical subdivisions.

INSERT INTO concept(concept_id, preferred_label)
VALUES (53391, 'Paper Industry Reference & Catalogs');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
VALUES (53391, 355, 0);

UPDATE lcc_range
SET concept_id = 53391
WHERE concept_id = 355
  AND ((start_code = 'TS1104' AND end_code = 'TS1105')
    OR (start_code = 'TS1210' AND end_code = 'TS1220'));

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (355, 'TS1080-1268.2');
