BEGIN;

DELETE FROM lcc_range
WHERE concept_id=13370 AND start_code='JS300' AND end_code='JS1583';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (13370,'JS300-JS1583');

COMMIT;
