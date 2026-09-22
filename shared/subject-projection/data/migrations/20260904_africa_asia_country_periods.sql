-- Reduce curated African and Asian country branches to direct period entries,
-- and remove Local History & Places throughout both regional trees. Complete
-- detail remains available in the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE regional_country(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT OR IGNORE INTO regional_country VALUES
 -- Africa
 ('unified:f52b29a0-8864-54b7-9460-3403aed7ba41'),('unified:311fcd05-f87d-5d51-a6df-b4b2c2ba5e05'),
 ('unified:29afcd41-3504-5d61-a87d-26a6450b0b25'),('unified:dd20ef8a-2f6f-58bd-acf2-832ecc022be2'),
 ('unified:6b79a347-32eb-56b1-a996-b6679b3863b7'),('unified:c95d96a5-eb2a-53b1-8ba1-afe20b672854'),
 ('unified:1a4b5b46-1205-5aa3-ac46-62072c7663bc'),('unified:df91e828-a780-53ee-b568-88f5e1ced94a'),
 ('unified:023b2c7b-7f50-545c-92ba-c12fb2a7cdda'),('unified:70f2280a-67f3-5a8b-8af3-e239b5c489c4'),
 ('unified:ae9a93f5-2f01-5b34-8573-fcce92028715'),('bokheim:history-lcc:ba334f81-0707-5e4b-b344-8fcbdb73031b'),
 ('bokheim:history-lcc-full:e83f39da-73d3-5e19-87c7-0a5498b47aae'),('bokheim:history-lcc-full:c4863d1b-2746-59d1-97a7-7e9ef93c9efb'),
 ('unified:5eb01a0a-ea26-5ba8-8a31-84d785c01058'),('unified:3ce46fca-4b03-5cd3-81ad-229f425ef319'),
 ('unified:e3ad1bf3-e78a-5b6a-b5f3-32a22be71da3'),('unified:4dde0914-c658-5618-8196-c6bb5f159e75'),
 ('unified:f04fe226-e7f6-5b9b-a15a-3a54214491c9'),('unified:377a38c7-0331-57cc-bfc0-fda307e8d377'),
 ('unified:fdc3ab70-111b-5a30-b825-d8207f6a329a'),('unified:c8bbd820-be93-5bd1-b173-e868678ac939'),
 ('unified:8101ec3f-509a-5f9a-b4a8-0279050b9d97'),('unified:c0dd725f-e53d-51de-9008-5663bc0b14f3'),
 ('unified:e4aaae39-a38f-5e54-a44d-c78c624c0d92'),('unified:adb0c9d0-cdf8-561e-a4ad-cad932f54ac1'),
 ('unified:7023e27f-5361-546b-b070-bc9660480634'),('unified:276de897-0df2-5379-af08-f560bfc10476'),
 ('bokheim:history-lcc-full:c6594240-6447-5df2-9d99-7ca028c97d41'),('unified:614064da-29bf-5c41-bc87-18f2218a0b20'),
 ('unified:3bd1ef93-676c-5e4b-97ae-92bb33ada2cd'),('unified:98e6bf33-7bd0-553b-aeb3-097f8ad888f3'),
 ('unified:f889b064-11c3-5778-b753-20954919876a'),('unified:4df06a56-4550-5207-8e92-a43acd1ca319'),
 ('unified:3a597bcf-cceb-589f-a7c3-a7457ab2ad0c'),('unified:71066a24-afd6-5224-8a72-1bd3d9bc2fa9'),
 ('unified:9f947214-284f-513d-a106-30bbf03dd9c8'),('unified:dde760e2-ddcb-5a67-819d-01c9b9b24cb3'),
 ('unified:a4bcb9b3-0237-5332-89bb-4c3ca70d0a99'),('bokheim:history-lcc-full:03b83db5-9f93-5690-afac-7fcf84567020'),
 ('unified:0bcd58b9-034e-53a7-82b8-1ce5e872cdab'),('unified:af68b079-0a66-5c6b-8eb6-ac58dfa7aede'),
 ('unified:269a0d85-4549-598d-aa34-53cf24eeb05e'),('unified:12cbc983-844a-56a4-a344-dbb1de18d7ef'),
 ('unified:a44d52b7-b5c5-5abf-a3e3-4aede0e6554e'),('unified:1225ad11-53f9-5202-81cb-11a8c2ec5596'),
 ('unified:046085fd-1dd2-5ba1-b76e-5f6035925b85'),
 -- Asia
 ('unified:3526bc96-ba65-57ae-8022-3a7348f32db6'),('bokheim:history-lcc-full:e3312677-a702-53c8-8d97-3da56ffc2389'),
 ('unified:cb87abd6-18df-59ca-bd22-dbfbcc0e6167'),('unified:8e5744c6-a5a9-5bf7-8021-bccada491087'),
 ('unified:28081db2-13aa-5953-ad57-0d155dec53b2'),('unified:f7728c0c-f2de-5596-836c-79dbf8c8bec3'),
 ('unified:0ba918c7-85ce-54fd-98ad-22519dc0583e'),('unified:d0f2acae-3c90-5512-b6aa-5433934c50a6'),
 ('unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d'),('unified:797de7a1-831c-5800-b5a2-04ca28a45e0b'),
 ('unified:da8cb1a3-5d71-5e93-909d-343f1d588a0e'),('bokheim:history-lcc-full:87917ebb-f063-537f-bf5f-c3b06799705f'),
 ('unified:0e8c1479-3ab3-5c3f-8c40-a8a09166c833'),('unified:5e8d7315-f8dd-5d7d-8307-fbd59528ca5c'),
 ('unified:80e7e55b-681d-5110-9666-4d9cf566e957'),('unified:872b31e1-e37e-50f3-86f2-292392a4ee4b'),
 ('unified:59a19860-ca1f-5b84-a017-931841482fbc'),('bokheim:history-lcc-full:da088234-8751-57b0-954f-3893b5f4f6da'),
 ('unified:65ed499a-2fbb-5fe5-a614-9e6279d2061f'),('bokheim:history-lcc-full:802e7561-3b7d-53df-bb15-dd6527ef5c21'),
 ('unified:c77345ab-a3c6-522b-b76f-376d659f1e7e');

DELETE FROM regional_country WHERE concept_id NOT IN (SELECT concept_id FROM concept);

CREATE TEMP TABLE regional_period(state_id TEXT NOT NULL, concept_id TEXT PRIMARY KEY, display_order INTEGER NOT NULL) WITHOUT ROWID;
INSERT OR IGNORE INTO regional_period
SELECT country.concept_id, period.concept_id, period.display_order
FROM regional_country country
JOIN concept_parent wrapper ON wrapper.parent_concept_id=country.concept_id
JOIN concept wrapper_item ON wrapper_item.concept_id=wrapper.concept_id
JOIN concept_parent period ON period.parent_concept_id=wrapper.concept_id
WHERE lower(wrapper_item.preferred_label)='by period';

INSERT OR IGNORE INTO regional_period
SELECT country.concept_id, child.concept_id, 10000 + child.display_order
FROM regional_country country
JOIN concept_parent child ON child.parent_concept_id=country.concept_id
JOIN concept item ON item.concept_id=child.concept_id
WHERE item.preferred_label GLOB '[0-9]*' OR lower(item.preferred_label) GLOB 'early to *';

CREATE TEMP TABLE country_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id FROM regional_country country
    JOIN concept_parent child ON child.parent_concept_id=country.concept_id
    UNION
    SELECT child.concept_id FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO country_detail
SELECT concept_id FROM detail WHERE concept_id NOT IN (SELECT concept_id FROM regional_period);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM regional_period)
  AND parent_concept_id IN (SELECT concept_id FROM country_detail UNION SELECT concept_id FROM regional_country);
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM regional_period)
  AND parent_concept_id IN (SELECT concept_id FROM regional_period);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT period.concept_id,period.state_id,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE concept_id=period.concept_id),0),
       period.display_order
FROM regional_period period;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM country_detail)
   OR parent_concept_id IN (SELECT concept_id FROM country_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM country_detail);

-- Remove any remaining Local History & Places branches in Africa and Asia.
CREATE TEMP TABLE regional_local_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
regional(concept_id) AS (
    VALUES ('bokheim:subject:history:africa'),('bokheim:subject:history:asia')
    UNION
    SELECT child.concept_id FROM concept_parent child
    JOIN regional parent ON parent.concept_id=child.parent_concept_id
),
local_root(concept_id) AS (
    SELECT item.concept_id FROM regional item JOIN concept c ON c.concept_id=item.concept_id
    WHERE lower(c.preferred_label)='local history & places'
),
detail(concept_id) AS (
    SELECT concept_id FROM local_root
    UNION
    SELECT child.concept_id FROM concept_parent child JOIN detail parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO regional_local_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM regional_local_detail)
   OR parent_concept_id IN (SELECT concept_id FROM regional_local_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM regional_local_detail);

DROP TABLE regional_local_detail;
DROP TABLE country_detail;
DROP TABLE regional_period;
DROP TABLE regional_country;
COMMIT;
