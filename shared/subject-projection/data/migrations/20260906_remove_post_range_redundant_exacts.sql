-- Parser-verified exact selectors already covered by ranges on the same concept.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
DELETE FROM source_selector WHERE system_id='lcc' AND (
 (concept_id=3375 AND selector IN ('DT644.8','DT648','DT649')) OR
 (concept_id=11566 AND selector IN ('DD286.3','DD286.4','DD286.5'))
);
COMMIT;
