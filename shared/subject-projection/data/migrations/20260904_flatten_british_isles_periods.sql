-- Reduce the curated British Isles chronology to broad eras beneath each
-- constituent country. Specific events, reigns, sites, and nested date ranges
-- remain available in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE british_country(
    concept_id TEXT PRIMARY KEY,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO british_country VALUES
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122', 10), -- England
    ('unified:02b271ae-547c-50c9-b03e-b334aec0c7d2', 20), -- Wales
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca', 30), -- Scotland
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22', 40); -- Ireland

CREATE TEMP TABLE british_period(
    country_id TEXT NOT NULL,
    concept_id TEXT PRIMARY KEY,
    preferred_label TEXT NOT NULL,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO british_period VALUES
    -- England
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:03d52510-06ca-549e-bb91-57ec2589334e','Early England, to 1066',10),
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:f40d656e-672e-56ed-be76-f523731bcaf6','Medieval England, 1066-1485',20),
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:6b88f61b-d35d-5bf2-8d26-1b6d9989e0d4','Tudor era, 1485-1603',30),
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:0da2172a-6a11-546e-a7e8-567a31d05858','17th century',40),
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:751cf1be-1e14-5420-9e22-90eea3a120f6','18th century',50),
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:d7e613af-4eba-5ddf-93e4-5cfe567f91a9','19th century',60),
    ('unified:2bb8dc99-b44c-56ef-abb6-c1cb2c0ef122','bokheim:history-lcc:2613aa58-3551-54ff-924b-6d6537316983','20th century',70),
    -- Wales
    ('unified:02b271ae-547c-50c9-b03e-b334aec0c7d2','bokheim:history-lcc-full:aa7b8e92-5b00-5ceb-bf04-81ab37b0e9ad','Early and medieval Wales',10),
    ('unified:02b271ae-547c-50c9-b03e-b334aec0c7d2','bokheim:history-lcc-full:dab5d10f-2649-51f4-87e6-c40227d6833f','Modern Wales',20),
    -- Scotland
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca','bokheim:history-lcc:7eebef62-074a-5d8d-967c-baf97125e257','Early and medieval Scotland, to 1603',10),
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca','bokheim:history-lcc:417123dd-672d-5524-b20f-57b459729290','Early modern Scotland, 1603-1707',20),
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca','bokheim:history-lcc:130e49c0-9127-5d0b-93f0-22c9c2f972e9','18th century',30),
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca','bokheim:history-lcc-full:00093720-4914-55e3-8081-960c59db8a6f','19th century',40),
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca','bokheim:history-lcc-full:66ebac06-f139-50ee-98ca-1637428689ae','20th century',50),
    ('unified:154e6411-517e-5b13-92cf-c259a0b9a0ca','bokheim:history-lcc-full:ca5dfd92-b032-5c8a-8e10-fd9ed9559e41','21st century',60),
    -- Ireland
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc-full:f22fa3bc-0bf9-5012-bede-f8787e8505fa','Early Ireland, to 1172',10),
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc:7359a81e-3863-55fb-b6f5-d3f19b79b682','Medieval and Tudor Ireland, 1172-1603',20),
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc:34ccd436-dea4-57e4-8fde-0a14a331e1f1','17th century',30),
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc:9561622e-62d8-566e-8578-c9442bf468a3','18th century',40),
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc:effe81db-6d79-5d45-8e4e-d36b242f76b2','19th century',50),
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc:7f4d7cb5-c550-514f-8d1f-b5386fd9711e','20th century',60),
    ('unified:6bd8eefe-24bf-5983-bc82-f1ca7373cc22','bokheim:history-lcc:9113d40a-4a4c-53ed-9a7d-b75026128b69','21st century',70);

-- Collect everything below the four countries. Retained periods are detached
-- before the remaining detail is removed.
CREATE TEMP TABLE british_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT edge.concept_id FROM british_country country
    JOIN concept_parent edge ON edge.parent_concept_id=country.concept_id
    UNION
    SELECT edge.concept_id FROM detail parent
    JOIN concept_parent edge ON edge.parent_concept_id=parent.concept_id
)
INSERT INTO british_detail SELECT concept_id FROM detail
WHERE concept_id NOT IN (SELECT concept_id FROM british_period);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM british_period)
  AND parent_concept_id IN (
      SELECT concept_id FROM british_detail
      UNION SELECT concept_id FROM british_country
  );

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT period.concept_id,period.country_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=period.concept_id),0),
       period.display_order
FROM british_period period;

UPDATE concept
SET preferred_label=(SELECT preferred_label FROM british_period WHERE british_period.concept_id=concept.concept_id)
WHERE concept_id IN (SELECT concept_id FROM british_period);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM british_detail)
   OR parent_concept_id IN (SELECT concept_id FROM british_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM british_detail);

-- England and Wales are now sibling countries directly beneath British Isles.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM british_country);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT country.concept_id,'unified:f3fa3554-e35d-512f-9115-0052e24458fc',0,country.display_order
FROM british_country country;

-- Remove the obsolete England-and-Wales wrapper if it is no longer referenced.
DELETE FROM concept_parent WHERE concept_id='unified:bbf558f9-bf2e-524b-ae54-10f7c8d8b2ae';
DELETE FROM concept WHERE concept_id='unified:bbf558f9-bf2e-524b-ae54-10f7c8d8b2ae';

DROP TABLE british_detail;
DROP TABLE british_period;
DROP TABLE british_country;
COMMIT;
