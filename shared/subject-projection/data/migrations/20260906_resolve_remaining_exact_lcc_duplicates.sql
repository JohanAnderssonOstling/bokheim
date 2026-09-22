-- Resolve the remaining cross-branch exact duplicates in favor of the more
-- specific curated subject.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector WHERE system_id='lcc' AND (
 (concept_id=4549 AND selector='BL2680') OR
 (concept_id=3357 AND selector='DT330') OR
 (concept_id=3418 AND selector='F106') OR
 (concept_id=5359 AND selector IN ('JS3000','JS3000.3','JS3000.7')) OR
 (concept_id=5281 AND selector='K3367') OR
 (concept_id=5317 AND selector IN ('KJC2635','KJC9799')) OR
 (concept_id=5318 AND selector='KJE6554') OR
 (concept_id=2845 AND selector='RA638') OR
 (concept_id=325 AND selector='TE298') OR
 (concept_id=4748 AND selector='Z686')
);

COMMIT;
