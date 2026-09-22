BEGIN;

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (52942,5233,1);

UPDATE lcc_selector SET concept_id=52942
WHERE concept_id=5233 AND selector='HD58';

UPDATE lcc_selector SET concept_id=12330
WHERE concept_id=5233 AND selector IN ('HD2321','HD2324','HD2326','HD2328');

UPDATE lcc_selector SET concept_id=5154
WHERE concept_id=5233 AND selector='HD2331-HD2336.35';

UPDATE lcc_selector SET concept_id=12333
WHERE concept_id=5233 AND selector='HD2337-HD2339';

UPDATE lcc_selector SET concept_id=12334
WHERE concept_id=5233 AND selector='HD2358-HD2360.5';

COMMIT;
