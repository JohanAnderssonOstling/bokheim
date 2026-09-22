-- PN6110.5-PN6120 is the official collections-of-drama span. Assign it to the
-- existing Anthologies leaf instead of enumerating its language/topic points on
-- the Drama parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector WHERE system_id='lcc' AND concept_id IN (3876,3880);
INSERT INTO source_selector VALUES(3880,'lcc','PN6110.5..PN6120.Z');
COMMIT;
