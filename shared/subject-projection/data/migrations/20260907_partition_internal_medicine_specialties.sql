-- Replace overlapping Internal Medicine Specialties ranges with uniquely owned
-- official disease-system bands.  Reuse existing medical leaves through
-- polyhierarchy and create only the two missing, high-use schedule concepts.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53305,'Nutritional Diseases'),
  (53306,'Diseases by Body Region');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53305,2718,0),
  (53306,2718,0);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT wanted.concept_id,2718,coalesce(max(existing.ordinal),-1)+1
FROM (
  SELECT 2557 concept_id UNION ALL
  SELECT 2602 UNION ALL
  SELECT 2610 UNION ALL
  SELECT 2653 UNION ALL
  SELECT 2655 UNION ALL
  SELECT 2694 UNION ALL
  SELECT 2907
) AS wanted
LEFT JOIN concept_parent AS existing USING(concept_id)
GROUP BY wanted.concept_id;

DELETE FROM lcc_range WHERE concept_id=2718;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('RC620','RC627',53305),
  ('RC627.5','RC632',2602),
  ('RC633','RC647',2694),
  ('RC648','RC665.2',2602),
  ('RC666','RC701.2',2557),
  ('RC705','RC779',2655),
  ('RC799','RC869',2610),
  ('RC870','RC923.2',2907),
  ('RC924','RC935',2653),
  ('RC936','RC951',53306);
