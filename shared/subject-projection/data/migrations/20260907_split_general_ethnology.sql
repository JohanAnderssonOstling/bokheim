-- Split the two high-use official spans from General Ethnology.  Once it has
-- children, the parent keeps exact headings but no permissive catch-all range.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53302,'Ethnology General Works & Textbooks'),
  (53303,'Ethnological Theory & Methods');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53302,12979,0),
  (53303,12979,0);

UPDATE lcc_selector SET concept_id=53302
WHERE concept_id=12979 AND selector='GN310-GN316';

UPDATE lcc_selector SET concept_id=53303
WHERE concept_id=12979 AND selector='GN345-GN348';

DELETE FROM lcc_range
WHERE concept_id=12979
  AND start_code='GN301'
  AND end_code='GN356.9';

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('GN310','GN316',53302),
  ('GN345','GN348',53303);
