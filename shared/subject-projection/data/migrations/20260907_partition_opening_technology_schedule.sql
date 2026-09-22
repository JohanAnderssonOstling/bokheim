-- Partition T1-T51 and remove permissive ranges from its nested communication
-- and directory nonleafs. Printed divisions remain exact on their parents.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53362,'Technology Reference & Catalogs'),
  (53363,'Philosophy of Technology'),
  (53364,'Technology Works & Reference Tables');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53362,5917,0),
  (53363,5918,0),
  (53364,5918,0);

DELETE FROM lcc_range WHERE concept_id IN (5917,5918,443,445);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('T1','T10.4',53362),
  ('T12.6','T13',53362),
  ('T14.7','T40',329),
  ('T44','T51',53364);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (5917,'T1-13'),
  (443,'T10.5-11.9'),
  (443,'T10.5'),
  (443,'T10.8'),
  (443,'T11.9'),
  (445,'T11.95-12.5'),
  (445,'T11.95'),
  (5918,'T14-51'),
  (53363,'T14'),
  (435,'T14.5'),
  (329,'T14.7-33');
