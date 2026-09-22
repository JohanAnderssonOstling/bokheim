-- Partition the QP schedule into its official major divisions. The existing
-- neurophysiology, animal-biochemistry, and pharmacology branches retain their
-- detailed descendants for subsequent residual partitioning.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53326,'Physiology Reference, Study & Research'),
  (53327,'Animal Life, Tissues & Blood'),
  (53328,'Circulation & Respiration'),
  (53329,'Nutrition, Digestion & Metabolism'),
  (53330,'Glands & Secretions'),
  (53331,'Urinary & Reproductive Physiology'),
  (53332,'Musculoskeletal Physiology'),
  (53333,'Experimental Pharmacology Reference & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53326,5043,0),
  (53327,5043,0),
  (53328,5043,0),
  (53329,5043,0),
  (53330,5043,0),
  (53331,5043,0),
  (53332,5043,0),
  (53333,5045,0);

DELETE FROM lcc_range
WHERE concept_id=5043
  AND start_code='QP1'
  AND end_code='QP981';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QP1','QP80',53326),
  ('QP81','QP99.5',53327),
  ('QP101','QP139',53328),
  ('QP141','QP185.3',53329),
  ('QP186','QP246.5',53330),
  ('QP247','QP285',53331),
  ('QP301','QP336',53332),
  ('QP901','QP910',53333);
