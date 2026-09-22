BEGIN;

-- Keep the published umbrella notation exact on the navigational parent.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5077,'HT101-HT395');

-- Route the official named notations into the leaf whose range contains them.
UPDATE lcc_selector SET concept_id=12299
WHERE concept_id=5077 AND selector IN
 ('HT101','HT105','HT107','HT108','HT108.5','HT109','HT110','HT151',
  'HT152','HT153','HT155','HT156');

UPDATE lcc_selector SET concept_id=12300
WHERE concept_id=5077 AND selector='HT161-HT165';

UPDATE lcc_selector SET concept_id=12301
WHERE concept_id=5077 AND selector IN ('HT170-HT178','HT185');

UPDATE lcc_selector SET concept_id=12302
WHERE concept_id=5077 AND selector='HT201-HT221';

UPDATE lcc_selector SET concept_id=12303
WHERE concept_id=5077 AND selector='HT231';

UPDATE lcc_selector SET concept_id=12304
WHERE concept_id=5077 AND selector IN
 ('HT241','HT251-HT265.2','HT281','HT319.92-HT320.22');

UPDATE lcc_selector SET concept_id=12305
WHERE concept_id=5077 AND selector='HT321';

UPDATE lcc_selector SET concept_id=12306
WHERE concept_id=5077 AND selector='HT330-HT334';

UPDATE lcc_selector SET concept_id=12307
WHERE concept_id=5077 AND selector='HT351';

UPDATE lcc_selector SET concept_id=12308
WHERE concept_id=5077 AND selector='HT361-HT384';

UPDATE lcc_selector SET concept_id=12309
WHERE concept_id=5077 AND selector='HT388';

-- This child previously had only an exact textual range, which left the broad
-- parent as the permissive owner. Make the actual leaf own its interval.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES ('HT165.5','HT169.9',597);

-- The children now cover the entire parent span; broad matching belongs to
-- those leaves, while the parent retains only its exact printed notation.
DELETE FROM lcc_range
WHERE concept_id=5077
  AND ((start_code='HT101' AND end_code='HT395')
    OR (start_code='HT111' AND end_code='HT150'));

COMMIT;
