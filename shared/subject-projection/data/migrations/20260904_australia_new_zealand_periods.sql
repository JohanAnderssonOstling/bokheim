-- Correct Australia and New Zealand to retain periods rather than provinces or
-- regional subdivisions. Remove Smaller Island Groups from curated Oceania.
-- Full data remains in the master taxonomy.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE anz_country(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO anz_country VALUES
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f'),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5');

CREATE TEMP TABLE anz_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id FROM anz_country country
 JOIN concept_parent child ON child.parent_concept_id=country.concept_id
 UNION
 SELECT child.concept_id FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO anz_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM anz_detail)
   OR parent_concept_id IN (SELECT concept_id FROM anz_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM anz_detail);

CREATE TEMP TABLE anz_period(
 country_id TEXT NOT NULL,
 concept_id TEXT PRIMARY KEY,
 display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO anz_period VALUES
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:0f2074fe-f21c-5cbf-b60c-9c0f064c948b',10),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:68edb7e7-a490-5e78-a597-bb64ee168c65',20),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:7be05031-c9f8-5f4f-8bca-83f6e16ceb1d',30),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc:82daaad9-f076-5615-b3dd-d3d1cd4c836e',10),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc:b3a1f68a-2467-54db-8b9a-dad1d680e98f',20),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc:71bf33c4-4ffe-5d33-b909-c27422465b37',30),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc:296ea2d5-cc2a-515c-ae56-d7d50d6db1e4',40),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc:1ce6296d-c443-542d-9659-544c75d0d6e8',50);

INSERT OR IGNORE INTO concept(concept_id,preferred_label)
SELECT source.concept_id,source.preferred_label FROM master.concept source
JOIN anz_period period ON period.concept_id=source.concept_id;
INSERT OR IGNORE INTO source_label(concept_id,label,ordinal)
SELECT source.concept_id,source.label,source.ordinal FROM master.source_label source
JOIN anz_period period ON period.concept_id=source.concept_id;
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT source.concept_id,source.system_id,source.selector,source.ordinal FROM master.source_selector source
JOIN anz_period period ON period.concept_id=source.concept_id;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT period.concept_id,period.country_id,0,period.display_order FROM anz_period period;

-- Remove Smaller Island Groups while retaining concepts also parented outside
-- that branch and their dependent descendants.
CREATE TEMP TABLE island_tree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE tree(concept_id) AS (
 VALUES ('unified:d9847b45-a98b-58b1-bcb9-35df308d7157')
 UNION
 SELECT child.concept_id FROM concept_parent child JOIN tree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO island_tree SELECT concept_id FROM tree;

CREATE TEMP TABLE island_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT tree.concept_id FROM island_tree tree
 JOIN concept_parent edge ON edge.concept_id=tree.concept_id
 WHERE tree.concept_id<>'unified:d9847b45-a98b-58b1-bcb9-35df308d7157'
   AND edge.parent_concept_id NOT IN (SELECT concept_id FROM island_tree)
 UNION
 SELECT child.concept_id FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM island_tree)
)
INSERT INTO island_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE island_remove(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO island_remove SELECT concept_id FROM island_tree
WHERE concept_id NOT IN (SELECT concept_id FROM island_shared);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM island_remove)
   OR parent_concept_id IN (SELECT concept_id FROM island_remove);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM island_remove);

DROP TABLE island_remove;
DROP TABLE island_shared;
DROP TABLE island_tree;
DROP TABLE anz_period;
DROP TABLE anz_detail;
DROP TABLE anz_country;
COMMIT;
DETACH DATABASE master;
