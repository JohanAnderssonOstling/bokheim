BEGIN;

DELETE FROM lcc_range WHERE concept_id=11473;

UPDATE lcc_selector SET concept_id=12855
WHERE concept_id=11473 AND selector IN (
 'BT198','BT199','BT199.5','BT205','BT207','BT210','BT222','BT225','BT295');

UPDATE lcc_selector SET concept_id=12857
WHERE concept_id=11473 AND selector IN ('BT520','BT520.72','BT575');

UPDATE lcc_selector SET concept_id=12858
WHERE concept_id=11473 AND selector IN ('BT580','BT582.2');

COMMIT;
