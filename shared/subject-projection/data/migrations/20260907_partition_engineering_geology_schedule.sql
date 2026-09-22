-- Move the misplaced TA703-TA705.4 division from the broad civil-navigation
-- concept into Engineering Geology and extract only its high-use topics.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53376,'Engineering Geology Reference & Regional Studies'),
  (53377,'Soil Mechanics Reference & Regional Studies'),
  (53378,'Rock Mechanics'),
  (53379,'Underground Construction');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53376,282,0),
  (53377,282,0),
  (53378,282,0),
  (53379,282,0);

DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (
  (5564,'TA703','TA705.4'),
  (282,'TA710.2','TA710.4')
);

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('TA703','TA705.4',53376),
  ('TA710.2','TA710.4',53377);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 53377,selector FROM lcc_selector
WHERE concept_id=282 AND selector IN ('TA710.A1','TA710.A15');
DELETE FROM lcc_selector
WHERE concept_id=282 AND selector IN ('TA710.A1','TA710.A15');

DELETE FROM lcc_selector
WHERE concept_id=282 AND selector IN ('TA706','TA706.3');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (282,'TA703-712'),
  (282,'TA706-706.5'),
  (282,'TA709'),
  (282,'TA709.5'),
  (282,'TA713'),
  (282,'TA714'),
  (282,'TA714.5'),
  (53377,'TA710'),
  (53378,'TA706'),
  (53378,'TA706.3'),
  (53379,'TA712');
