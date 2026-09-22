-- Replace permissive ranges on detailed nonleaf chemistry branches with
-- residual leaves. Existing exact specialist selectors override these broad
-- residual spans without creating competing range ownership.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53337,'Analytical Chemistry Reference & Methods'),
  (53338,'Inorganic Chemistry Reference, Study & Compounds');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53337,4804,0),
  (53338,4806,0);

DELETE FROM lcc_range WHERE concept_id IN (4804,4806);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QD71','QD80.Z',53337),
  ('QD99','QD100.Z',53337),
  ('QD118','QD129.Z',53337),
  ('QD140','QD142',53337),
  ('QD146','QD160.Z',53338),
  ('QD169.1','QD170.Z',53338),
  ('QD172.1','QD188.Z',53338),
  ('QD193.1','QD197',53338);
