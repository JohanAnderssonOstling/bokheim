BEGIN;
-- Each selector below is already owned by the same leaf's permissive range;
-- retaining both representations only inflates selector counts.
DELETE FROM lcc_selector WHERE concept_id=12601 AND selector IN ('JA81','JA83');
DELETE FROM lcc_selector WHERE concept_id=13275 AND selector='HV6626.54';
DELETE FROM lcc_selector WHERE concept_id=52928 AND selector IN ('HV6773','HV6773.15');
COMMIT;
