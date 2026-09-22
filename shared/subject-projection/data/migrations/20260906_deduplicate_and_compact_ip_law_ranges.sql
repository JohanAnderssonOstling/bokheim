-- The parent and child duplicated the same selector set. Keep ownership on the
-- specific child and compact only uninterrupted intellectual-property blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE concept_id=5616 AND system_id='lcc';

DELETE FROM source_selector WHERE concept_id=13314 AND system_id='lcc' AND (
 (selector>='KJC2655.2' AND selector<='KJC2700') OR
 (selector>='KJE2655.2' AND selector<='KJE2700') OR
 (selector>='KK2726' AND selector<='KK2833')
);
INSERT INTO source_selector VALUES
 (13314,'lcc','KJC2655.2..KJC2700'),
 (13314,'lcc','KJE2655.2..KJE2700'),
 (13314,'lcc','KK2726..KK2833');

COMMIT;
