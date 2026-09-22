-- Move residual reference, general-works, and teaching spans to one broad leaf;
-- route nuclear aircraft propulsion to the existing Aircraft Engines leaf.

INSERT INTO concept(concept_id, preferred_label)
VALUES (53397, 'Aeronautical Reference & Education');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal)
VALUES (53397, 405, 0);

UPDATE lcc_range
SET concept_id = 53397
WHERE concept_id = 405
  AND ((start_code = 'TL500' AND end_code = 'TL504')
    OR (start_code = 'TL544' AND end_code = 'TL551')
    OR (start_code = 'TL560' AND end_code = 'TL560.42'));

UPDATE lcc_range
SET concept_id = 8948
WHERE concept_id = 405
  AND start_code = 'TL708'
  AND end_code = 'TL708.3';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (405, 'TL500-504'),
  (405, 'TL544-551'),
  (405, 'TL560-560.42'),
  (405, 'TL708-708.3'),
  (405, 'TL545');
