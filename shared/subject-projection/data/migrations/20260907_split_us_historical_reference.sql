BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51133,'U.S. Historiography'),
  (51134,'Teaching U.S. History'),
  (51135,'Philosophy of U.S. History'),
  (51136,'U.S. Collective Biography'),
  (51137,'U.S. General Histories'),
  (51138,'U.S. Historical Geography');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51133,13699,0),
  (51134,13699,0),
  (51135,13699,0),
  (51136,13699,0),(51136,51001,1),
  (51137,13699,0),
  (51138,13699,0);

DELETE FROM lcc_range
WHERE concept_id=13699 AND class_letters='E'
  AND start_value='171' AND end_value='179.52';

-- Periodicals, societies, sources, documents, dictionaries, encyclopedias,
-- and chronological tables remain direct reference material on the parent.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','171','174.999',13699),
  ('E','175','175.799',51133),
  ('E','175.8','175.899',51134),
  ('E','175.9','175.999',51135),
  ('E','176','176.999',51136),
  ('E','178','179.499',51137),
  ('E','179.5','179.52',51138);

COMMIT;
