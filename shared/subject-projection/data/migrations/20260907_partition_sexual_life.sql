BEGIN;

UPDATE lcc_selector SET concept_id=12400
WHERE concept_id=5250 AND selector IN ('HQ18.5','HQ18.6');

UPDATE lcc_selector SET concept_id=12401
WHERE concept_id=5250 AND selector='HQ31-HQ64';

UPDATE lcc_selector SET concept_id=12402
WHERE concept_id=5250 AND selector IN ('HQ66','HQ67');

UPDATE lcc_selector SET concept_id=12403
WHERE concept_id=5250 AND selector IN ('HQ72.8','HQ74-HQ74.2');

UPDATE lcc_selector SET concept_id=12404
WHERE concept_id=5250 AND selector IN ('HQ71-HQ72','HQ79');

UPDATE lcc_selector SET concept_id=12405
WHERE concept_id=5250 AND selector='HQ101-HQ440.7';

UPDATE lcc_selector SET concept_id=12406
WHERE concept_id=5250 AND selector='HQ444-HQ445';

UPDATE lcc_selector SET concept_id=12407
WHERE concept_id=5250 AND selector='HQ447';

UPDATE lcc_selector SET concept_id=12408
WHERE concept_id=5250 AND selector='HQ449';

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HQ12','HQ30.7',12400),
 ('HQ72.8','HQ78.9',12403);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5250,'HQ12-HQ449');

DELETE FROM lcc_range WHERE concept_id=5250;

COMMIT;
