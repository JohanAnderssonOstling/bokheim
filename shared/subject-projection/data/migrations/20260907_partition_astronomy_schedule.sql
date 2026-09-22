-- Replace the QB umbrella with official high-use divisions.  Use exact points
-- where a navigational topic has a separately ranged child.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53318,'Astronomy Reference & Institutions'),
  (53319,'Cosmochemistry');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53318,4780,0),
  (53319,4780,0);

DELETE FROM lcc_range
WHERE concept_id=4780
  AND start_code='QB1'
  AND end_code='QB991';

DELETE FROM lcc_range
WHERE concept_id=5028
  AND start_code='QB4'
  AND end_code='QB4.9';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QB1','QB139',53318),
  ('QB450','QB450.5',53319);

INSERT INTO lcc_selector(concept_id,selector)
VALUES
  (5028,'QB4'),
  (5028,'QB4.9'),
  (4784,'QB494.2');

-- Wholly contained by broader ranges on the same subjects.
DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (4785,'QB299','QB299.5'),
  (7605,'QB600','QB701')
);
