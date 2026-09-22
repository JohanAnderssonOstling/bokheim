BEGIN;

UPDATE lcc_selector SET concept_id=13145
WHERE concept_id=5093 AND selector IN ('HB3711','HB3714');

UPDATE lcc_selector SET concept_id=13146
WHERE concept_id=5093 AND selector='HB3716-HB3717';

UPDATE lcc_selector SET concept_id=13147
WHERE concept_id=5093 AND selector IN ('HB3718-HB3728.52','HB3729');

UPDATE lcc_selector SET concept_id=13148
WHERE concept_id=5093 AND selector='HB3730-HB3730.2';

UPDATE lcc_selector SET concept_id=13149
WHERE concept_id=5093 AND selector='HB3732-HB3732.2';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (5093,'HB3711-HB3840');

DELETE FROM lcc_range
WHERE concept_id=5093 AND start_code='HB3711' AND end_code='HB3840';

COMMIT;
