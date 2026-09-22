-- Remove permissive ranges from the two detailed nonleaf QP branches. Exact
-- specialist headings continue to override the residual leaf ranges.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53334,'Neurophysiology Reference, Study & Methods'),
  (53335,'Animal Biochemistry Reference, Study & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53334,5046,0),
  (53335,5044,0);

DELETE FROM lcc_range
WHERE concept_id=5046
  AND (start_code,end_code) IN (
    ('QP351','QP495'),
    ('QP355','QP355.2')
  );

DELETE FROM lcc_range
WHERE concept_id=5044
  AND (start_code,end_code) IN (
    ('QP501','QP801'),
    ('QP514','QP514.2'),
    ('QP518','QP519')
  );

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QP351','QP353.2',53334),
  ('QP353.5','QP359.9',53334),
  ('QP501','QP510.9',53335),
  ('QP512','QP519.6',53335),
  ('QP520','QP524.9',53335);
