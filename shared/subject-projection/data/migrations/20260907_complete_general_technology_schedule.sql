-- Complete Engineering Drawing without leaving permissive ranges on its
-- nonleaf or on the Computer-Aided Drafting nonleaf.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53368,'Engineering Drawing Reference & Methods'),
  (53369,'Computer-Aided Drafting Software');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53368,449,0),
  (53369,9867,0);

DELETE FROM lcc_range WHERE concept_id IN (449,9867);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('T351','T361.999',53368),
  ('T385','T386.Z',53369);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (449,'T351-385'),
  (449,'T371'),
  (449,'T375'),
  (449,'T377'),
  (9867,'T385-386.Z');

-- Every occupied T division now has a child owner; preserve the printed whole
-- schedule exactly on the root instead of keeping a permissive umbrella.
DELETE FROM lcc_range
WHERE concept_id=5563
  AND start_code='T1'
  AND end_code='T995';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5563,'T1-995');
