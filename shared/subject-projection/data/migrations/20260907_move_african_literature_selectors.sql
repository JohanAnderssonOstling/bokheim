BEGIN;
UPDATE lcc_selector SET concept_id=8718
WHERE concept_id=3711 AND selector IN ('PL8009.5','PL8011');
COMMIT;
