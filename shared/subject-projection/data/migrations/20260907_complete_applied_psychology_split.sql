-- Complete the high-use BF636-BF637 split.  Exact BF636/BF637 remain broad
-- fallbacks on the nonleaf; the permissive range is no longer needed.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53307,'Persuasion'),
  (53308,'Personal Change');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53307,5384,0),
  (53308,5384,0),
  (5185,5384,1),
  (51346,5384,1);

INSERT INTO lcc_selector(concept_id,selector)
VALUES
  (5384,'BF636'),
  (5384,'BF637'),
  (11206,'BF636.5-.7'),
  (5185,'BF637.L4'),
  (53307,'BF637.P4'),
  (53308,'BF637.C4'),
  (51346,'BF637.B85');

DELETE FROM lcc_range
WHERE concept_id=5384
  AND start_code='BF636'
  AND end_code='BF637.2';
