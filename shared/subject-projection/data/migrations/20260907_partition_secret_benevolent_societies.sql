BEGIN;

UPDATE lcc_selector SET concept_id=13210
WHERE concept_id=5517 AND selector<>'HS'
  AND CAST(substr(selector,3) AS REAL) BETWEEN 1 AND 100.9;

UPDATE lcc_selector SET concept_id=13211
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 101 AND 350.9;

UPDATE lcc_selector SET concept_id=13212
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 351 AND 949.9;

UPDATE lcc_selector SET concept_id=13213
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 950 AND 1199.9;

UPDATE lcc_selector SET concept_id=13214
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 1200 AND 1499.9;

UPDATE lcc_selector SET concept_id=13215
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 1500 AND 2499.9;

UPDATE lcc_selector SET concept_id=13216
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 2500 AND 3249.9;

UPDATE lcc_selector SET concept_id=13217
WHERE concept_id=5517
  AND CAST(substr(selector,3) AS REAL) BETWEEN 3250 AND 3371;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5517,'HS1-HS3371');

DELETE FROM lcc_range WHERE concept_id=5517;

COMMIT;
