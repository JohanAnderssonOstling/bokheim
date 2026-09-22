BEGIN;

UPDATE lcc_selector SET concept_id=12772
WHERE concept_id=5235 AND selector IN
 ('HD101','HD103','HD105','HD107.7','HD107.8','HD108-HD108.8',
  'HD109-HD109.5','HD110-HD110.5','HD111','HD113-HD156');

UPDATE lcc_selector SET concept_id=12775
WHERE concept_id=5235 AND selector IN ('HD1286-HD1289','HD1290-HD1291');

UPDATE lcc_selector SET concept_id=12776
WHERE concept_id=5235 AND selector IN ('HD1326-HD1329','HD1330-HD1331');

UPDATE lcc_selector SET concept_id=12777
WHERE concept_id=5235 AND selector IN ('HD1332-HD1333.5','HD1334-HD1335');

UPDATE lcc_selector SET concept_id=12778
WHERE concept_id=5235 AND selector='HD1336-HD1339';

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES ('HD166','HD1138.9',12773);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5235,'HD101-HD1395.5');

DELETE FROM lcc_range WHERE concept_id=5235;

COMMIT;
