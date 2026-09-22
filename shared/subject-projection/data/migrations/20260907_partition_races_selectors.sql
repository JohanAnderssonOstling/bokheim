BEGIN;

UPDATE lcc_selector SET concept_id=13068
WHERE concept_id=5075 AND selector IN
 ('HT1501','HT1503','HT1505','HT1506','HT1507','HT1521','HT1523');

UPDATE lcc_selector SET concept_id=13069
WHERE concept_id=5075 AND selector='HT1530.2-HT1561';

UPDATE lcc_selector SET concept_id=13070
WHERE concept_id=5075 AND selector='HT1574.2-HT1595';

-- This interval spans every child and therefore cannot permissively belong to
-- Race History. Preserve the published umbrella only as an exact parent code.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5075,'HT1501-HT1595.22');

DELETE FROM lcc_range
WHERE concept_id=13068
  AND start_code='HT1501' AND end_code='HT1595.22';

COMMIT;
