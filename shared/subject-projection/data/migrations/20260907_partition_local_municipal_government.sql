BEGIN;

-- Route every specific JS selector by the established geographic intervals.
UPDATE lcc_selector SET concept_id=13368
WHERE concept_id=5359 AND selector<>'JS'
  AND CAST(substr(selector,3) AS REAL) BETWEEN 39 AND 113;

UPDATE lcc_selector SET concept_id=13369
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 141 AND 271;

UPDATE lcc_selector SET concept_id=13374
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 1701 AND 1819;

UPDATE lcc_selector SET concept_id=13375
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 1830 AND 2058.9;

UPDATE lcc_selector SET concept_id=13376
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 2061 AND 2778;

UPDATE lcc_selector SET concept_id=13378
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 6950 AND 7520;

UPDATE lcc_selector SET concept_id=13379
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 7525 AND 7819.9;

UPDATE lcc_selector SET concept_id=13380
WHERE concept_id=5359
  AND CAST(substr(selector,3) AS REAL) BETWEEN 7820 AND 8490.9;

-- Put permissive ownership on mutually exclusive leaves. Germany retains its
-- existing JS5301-JS5598 interval, so Europe is split around it.
DELETE FROM lcc_range WHERE concept_id=5359;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JS3000','JS5300.9',13377),
 ('JS5598.1','JS6949.89',13377);

-- Preserve only the genuinely cross-child printed European umbrella exactly.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
 (5359,'JS3000-JS6949.89'),
 (13370,'JS300-JS1583');

-- SH400 is the opening of Angling, not European local government. Fold the
-- misplaced fragments into the adjacent Angling History & Reference range.
DELETE FROM lcc_range WHERE concept_id=13377 AND start_code LIKE 'SH%';
UPDATE lcc_range SET start_code='SH400'
WHERE concept_id=13123 AND start_code='SH401' AND end_code='SH446.9';

COMMIT;
