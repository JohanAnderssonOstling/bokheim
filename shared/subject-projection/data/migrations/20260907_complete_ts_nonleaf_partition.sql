-- Complete the TS schedule: partition meaningful production and leather
-- children, consolidate low-use rubber subdivisions, and remove the class-wide
-- Industrial Production umbrella range.

INSERT INTO concept(concept_id, preferred_label) VALUES
  (53392, 'Production Management Reference & Methods'),
  (53393, 'Leather Industry Reference & Practice');

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal) VALUES
  (53392, 356, 0),
  (53393, 352, 0);

DELETE FROM lcc_range
WHERE concept_id = 356;

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TS155',    'TS155.39', 53392),
  ('TS155.51', 'TS155.59', 53392),
  ('TS155.68', 'TS155.69', 53392),
  ('TS169.1',  'TS169.99', 53392),
  ('TS175.51', 'TS175.99', 53392),
  ('TS155.7',  'TS155.79', 8205),
  ('TS183.61', 'TS183.99', 9134);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (356, 'TS155-194');

-- Rubber subdivisions have no topic at or above the expansion threshold.
DELETE FROM concept_parent
WHERE parent_concept_id = 357
  AND concept_id IN (7320, 7563, 7996, 7997, 9523);

DELETE FROM lcc_range
WHERE concept_id IN (357, 7997);

DELETE FROM lcc_selector
WHERE concept_id IN (7320, 7563, 7996, 9523);

DELETE FROM concept
WHERE concept_id IN (7320, 7563, 7996, 7997, 9523)
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
VALUES ('TS1870', 'TS1935', 357);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (357, 'TS1891'),
  (357, 'TS1892'),
  (357, 'TS1912'),
  (357, 'TS1925');

-- Leather retains its established history, shoemaking, and saddlery leaves.
DELETE FROM lcc_range
WHERE concept_id = 352;

INSERT INTO lcc_range(start_code, end_code, concept_id) VALUES
  ('TS940',   'TS949.99', 53393),
  ('TS959.1', 'TS988.99', 53393),
  ('TS1025.1','TS1029.99',53393),
  ('TS1035.1','TS1047',   53393);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (352, 'TS940-1047');

-- The broad class remains available for explicitly printed classifications,
-- while numbered schedules are owned only by their specific leaves.
DELETE FROM lcc_range
WHERE concept_id = 361
  AND start_code = 'TS1'
  AND end_code = 'TS2301';

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (361, 'TS1-2301');
