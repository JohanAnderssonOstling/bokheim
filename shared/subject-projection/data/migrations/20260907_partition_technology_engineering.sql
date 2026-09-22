-- Partition the high-use T schedule into existing topical leaves and one
-- official museum leaf.  The TA schedule already has precise owners elsewhere.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53304,'Technology Museums');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53304,5563,0);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('T1','T13',5917),
  ('T14','T51',5918),
  ('T179','T183',53304);

INSERT INTO lcc_selector(concept_id,selector)
VALUES (5563,'T185');

DELETE FROM lcc_range
WHERE concept_id=5563
  AND (start_code,end_code) IN (
    ('T1','T995'),
    ('TA1','TA2040')
  );
