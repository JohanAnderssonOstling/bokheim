-- Route regional three-digit DDC codes to the agreed existing History parents.
-- 972 uses Latin America as the existing shared parent for Middle America.
-- Per curation policy, retain the two-country 942 and 962 destinations.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

UPDATE ddc_selector SET concept_id=3552 WHERE selector='943' AND concept_id=3524;
UPDATE ddc_selector SET concept_id=3614 WHERE selector='946' AND concept_id=3613;
UPDATE ddc_selector SET concept_id=3522 WHERE selector='947' AND concept_id=3595;
UPDATE ddc_selector SET concept_id=3385 WHERE selector='972' AND concept_id=3391;

COMMIT;
