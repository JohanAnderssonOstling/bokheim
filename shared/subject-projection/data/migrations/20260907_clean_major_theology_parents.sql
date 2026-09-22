BEGIN;

WITH wanted(label) AS (VALUES
 ('Kingdom of God'),
 ('Christianity & Palestine'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,4581,0 FROM concept WHERE preferred_label='Kingdom of God';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,5801,0 FROM concept WHERE preferred_label='Christianity & Palestine';

UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Kingdom of God')
WHERE concept_id=4581 AND selector='BT94';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Christianity & Palestine')
WHERE concept_id=4581 AND selector='BT93.8';

-- The parent retains only the exact class-level BT selector.

-- Jewish beliefs: existing children cover the complete doctrinal spans.
DELETE FROM lcc_range WHERE concept_id=5870;
UPDATE concept SET preferred_label='Jewish Messianism, Humanity & Sin' WHERE concept_id=13028;
UPDATE lcc_selector SET concept_id=13025
WHERE concept_id=5870 AND selector='BM526';
UPDATE lcc_selector SET concept_id=13027
WHERE concept_id=5870 AND selector IN
 ('BM603','BM607','BM610','BM612','BM612.5','BM612.7');
UPDATE lcc_selector SET concept_id=13028
WHERE concept_id=5870 AND selector IN ('BM627','BM630');

-- Buddhist teachings and doctrine.
DELETE FROM lcc_range WHERE concept_id=5751;
UPDATE lcc_selector SET concept_id=13016
WHERE concept_id=5751 AND selector IN ('BQ4155','BQ4160','BQ4165','BQ4170');
UPDATE lcc_selector SET concept_id=13018
WHERE concept_id=5751 AND selector IN ('BQ4600','BQ4605');
UPDATE lcc_selector SET concept_id=13019
WHERE concept_id=5751 AND selector IN
 ('BQ4620','BQ4625','BQ4630','BQ4635','BQ4640','BQ4645','BQ4648','BQ4650');
UPDATE lcc_selector SET concept_id=13020
WHERE concept_id=5751 AND selector IN ('BQ6200','BQ6210','BQ6220','BQ6230');

-- Christian apologetics and interfaith theology.
DELETE FROM lcc_range WHERE concept_id=5801;
UPDATE lcc_selector SET concept_id=13048
WHERE concept_id=5801 AND selector IN ('BT1095','BT1105');
UPDATE lcc_selector SET concept_id=13053
WHERE concept_id=5801 AND selector IN ('BT93','BT93.5','BT93.55','BT93.6');

-- Buddhist sacred texts and literature.
DELETE FROM lcc_range WHERE concept_id=5753;
UPDATE lcc_selector SET concept_id=13218
WHERE concept_id=5753 AND selector IN ('BQ1010','BQ1020');
UPDATE lcc_selector SET concept_id=13219
WHERE concept_id=5753 AND selector IN
 ('BQ1100','BQ1105','BQ1107','BQ1110','BQ1112','BQ1113','BQ1115',
  'BQ1115.5','BQ1130','BQ1138');

COMMIT;
