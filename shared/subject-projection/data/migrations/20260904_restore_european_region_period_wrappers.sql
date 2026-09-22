-- Restore By period grouping for European regional subdivisions. Country
-- periods remain direct children. Wrapper definitions come from master.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE region_wrapper(
 region_id TEXT PRIMARY KEY,
 wrapper_id TEXT NOT NULL UNIQUE
) WITHOUT ROWID;
INSERT INTO region_wrapper VALUES
 ('unified:68284cc9-f9d0-5474-a92c-caabd008b66e','unified:221d53bc-f64c-53a2-9ab0-b05ed6663a6c'),
 ('unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec','bokheim:lcc-detail:5f9bb5a5-ccd1-595e-a4ab-9d8eb3916923'),
 ('unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad','unified:3f539c15-67eb-5735-8dfb-819a1c1476fb'),
 ('unified:d6daa986-0fa4-5394-945e-614d7b09d3ca','unified:ea45ab61-b6a1-51e6-b034-3ee2ce84f3b1');

INSERT OR IGNORE INTO concept(concept_id,preferred_label)
SELECT source.concept_id,source.preferred_label
FROM master.concept source
JOIN region_wrapper mapping ON mapping.wrapper_id=source.concept_id;
INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT source.concept_id,source.label,source.ordinal
FROM master.source_label source
JOIN region_wrapper mapping ON mapping.wrapper_id=source.concept_id;
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT source.concept_id,source.system_id,source.selector,source.ordinal
FROM master.source_selector source
JOIN region_wrapper mapping ON mapping.wrapper_id=source.concept_id;

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT mapping.wrapper_id,mapping.region_id,0,master_edge.display_order
FROM region_wrapper mapping
JOIN master.concept_parent master_edge
  ON master_edge.concept_id=mapping.wrapper_id
 AND master_edge.parent_concept_id=mapping.region_id;

CREATE TEMP TABLE region_period(
 concept_id TEXT NOT NULL,
 region_id TEXT NOT NULL,
 wrapper_id TEXT NOT NULL,
 display_order INTEGER NOT NULL,
 PRIMARY KEY(concept_id,region_id)
) WITHOUT ROWID;
INSERT INTO region_period
SELECT period.concept_id,mapping.region_id,mapping.wrapper_id,period.display_order
FROM region_wrapper mapping
JOIN master.concept_parent period ON period.parent_concept_id=mapping.wrapper_id
JOIN concept_parent current
  ON current.concept_id=period.concept_id
 AND current.parent_concept_id=mapping.region_id;

DELETE FROM concept_parent
WHERE (concept_id,parent_concept_id) IN (
 SELECT concept_id,region_id FROM region_period
);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT period.concept_id,period.wrapper_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent existing
                 WHERE existing.concept_id=period.concept_id),0),
       period.display_order
FROM region_period period;

DROP TABLE region_period;
DROP TABLE region_wrapper;
COMMIT;
DETACH DATABASE master;
