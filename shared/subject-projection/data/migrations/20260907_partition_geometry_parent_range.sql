-- Specialist leaves cover the occupied Geometry schedule after QA450. Preserve
-- the printed whole division exactly and range only its opening material.

INSERT INTO concept(concept_id,preferred_label)
VALUES (53358,'Geometry Reference, History & Foundations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (53358,4939,0);

DELETE FROM lcc_range WHERE concept_id=4939;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES ('QA440','QA450.Z',53358);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (4939,'QA440-699');

-- QA640.7 is the explicit Discrete Geometry heading, so the preceding convex
-- interval stops immediately below it.
UPDATE lcc_range
SET end_code='QA640.69'
WHERE concept_id=8570
  AND start_code='QA639.5'
  AND end_code='QA640.7';

-- Wholly contained by the broader range on the same destination.
DELETE FROM lcc_range
WHERE concept_id=4971
  AND (start_code,end_code) IN (
    ('QA612','QA612.8'),
    ('QA613','QA613.8'),
    ('QA614','QA614.97')
  );
