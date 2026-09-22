-- Repair the History & Philosophy split and add only the high-use isolated QA
-- headings revealed by the root-fallback audit.

UPDATE lcc_range
SET end_code='QA8.99'
WHERE concept_id=4947
  AND start_code='QA8'
  AND end_code='QA8.Z';

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QA21','QA27',4947);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (13694,'QA39.3');

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53360,'Mathematical Problem Solving'),
  (53361,'Sequences & Series');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53360,5722,0),
  (53361,5719,0);

INSERT INTO lcc_selector(concept_id,selector)
VALUES
  (53360,'QA63'),
  (53361,'QA292'),
  (53361,'QA295');
