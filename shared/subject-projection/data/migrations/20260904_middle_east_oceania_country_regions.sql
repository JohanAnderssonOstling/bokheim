-- Reduce curated Middle Eastern and Oceanian country branches to countries
-- plus broad regional subdivisions. Fine-grained detail remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE regional_country(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT OR IGNORE INTO regional_country VALUES
 ('unified:0756cf48-f424-58af-b65f-94d14994861d'),
 ('unified:e39978ec-a73a-5949-8c89-f2f413386e3e'),
 ('unified:34ad48ff-92de-5584-8307-c2205c6e0f5d'),
 ('unified:51d2665b-797b-5354-9470-a36fa9e65fb9'),
 ('unified:2f776a9d-cdd5-5ada-b28c-888db2077187'),
 ('unified:356028f8-c8cd-54c1-879e-2af2da6a204c'),
 ('unified:e37e872e-1221-57a6-b5f9-74a307f67b3d'),
 ('unified:e846c8e2-891e-51b4-adf6-eae0842628eb'),
 ('unified:b1da6c49-e4e6-5dca-b82a-8f37a1793e80'),
 ('bokheim:history-lcc-full:373ec740-0e68-50ce-8be1-ac6f139cfb65'),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f'),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5'),
 ('bokheim:history-lcc-full:e20a3df9-ed4b-5b80-8ead-e22be651e0d5'),
 ('bokheim:history-lcc-full:3981f657-f9c5-51f6-a6f3-736a332297b7'),
 ('bokheim:history-lcc-full:a4b5f8ea-8e88-536a-a3f9-6ff0e35f5042'),
 ('bokheim:history-lcc-full:e03070a2-b2d0-5abf-9351-8e090eb37adf');
DELETE FROM regional_country WHERE concept_id NOT IN (SELECT concept_id FROM concept);

CREATE TEMP TABLE retained_region(
 country_id TEXT NOT NULL,
 concept_id TEXT PRIMARY KEY,
 display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO retained_region VALUES
 -- Australia: states, territories, and broad geographic regions.
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','unified:2d06acd2-04c1-5676-9daa-c5afeaf397c5',10),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','unified:37eaeea9-5f36-5861-924a-6f15bdb26325',20),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','unified:9db41356-f4bb-5e06-b1be-35474a4b71c1',30),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','unified:ebef9e44-6068-547e-b09b-f007bca14219',40),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','unified:f388041a-2c8a-5c25-8ffa-aa69137faa67',50),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','unified:ff593e09-4ff8-58e9-b0bd-1844c5738fdc',60),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:1a6bd17e-0292-552c-864f-9fdc8fff48ba',70),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:55850c99-fa24-5bc1-942a-02456c018552',80),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:0ea2eec9-82bc-5969-82a7-c91cc3eb1641',90),
 ('unified:f5299b7d-b39d-5d2b-846f-75e1e6c5395f','bokheim:history-lcc-full:18002d31-c173-5e6c-930d-1c514798e389',100),
 -- New Zealand: major islands and historical/provincial regions.
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:6ed9eace-9792-5acc-9e86-4cb933e2a472',10),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:49ea4802-7442-5b54-a57a-8c1d779fb33d',20),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:3ce9300d-e882-5919-a76d-3b02d5ad64ae',30),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:6c1feda2-8b67-5bdd-afab-bd603a9ef7e7',40),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:563f97c6-543d-5ad6-b7dc-d4b7c0f2195b',50),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:6d2f722f-d9db-5b0c-bc41-b78d80dd4565',60),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:afe32f76-dad0-52e6-bab5-844668803cf8',70),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:a0f2d874-701a-5595-ba5e-1215d080c8dd',80),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:2e0daa5a-e98d-5b58-a5f4-7480baf82831',90),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:e8ba6f81-40d2-5146-b6f9-6924568761d6',100),
 ('unified:f282e215-0300-5f01-a6e4-2a76f232dfc5','bokheim:history-lcc-full:e07808a9-5183-57be-b1fb-065436428a63',110),
 -- Iran retains the Persian Gulf as its sole broad regional entry.
 ('unified:34ad48ff-92de-5584-8307-c2205c6e0f5d','bokheim:history-lcc-full:d48811be-4bf2-5c31-bab5-48d5686ec83c',10);
DELETE FROM retained_region WHERE concept_id NOT IN (SELECT concept_id FROM concept);

CREATE TEMP TABLE country_subtree(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT child.concept_id FROM regional_country country
 JOIN concept_parent child ON child.parent_concept_id=country.concept_id
 UNION
 SELECT child.concept_id FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO country_subtree SELECT concept_id FROM detail;

CREATE TEMP TABLE country_shared(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE preserved(concept_id) AS (
 SELECT subtree.concept_id FROM country_subtree subtree
 JOIN concept_parent edge ON edge.concept_id=subtree.concept_id
 WHERE edge.parent_concept_id NOT IN (SELECT concept_id FROM country_subtree)
   AND edge.parent_concept_id NOT IN (SELECT concept_id FROM regional_country)
 UNION
 SELECT child.concept_id FROM preserved parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
 WHERE child.concept_id IN (SELECT concept_id FROM country_subtree)
)
INSERT INTO country_shared SELECT concept_id FROM preserved;

CREATE TEMP TABLE country_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO country_detail
SELECT concept_id FROM country_subtree
WHERE concept_id NOT IN (SELECT concept_id FROM retained_region)
  AND concept_id NOT IN (SELECT concept_id FROM country_shared);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM retained_region)
  AND parent_concept_id IN (SELECT concept_id FROM country_detail UNION SELECT concept_id FROM regional_country);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT region.concept_id,region.country_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=region.concept_id),0),
       region.display_order
FROM retained_region region;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM country_detail)
   OR parent_concept_id IN (SELECT concept_id FROM country_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM country_detail);

DROP TABLE country_detail;
DROP TABLE country_shared;
DROP TABLE country_subtree;
DROP TABLE retained_region;
DROP TABLE regional_country;
COMMIT;
