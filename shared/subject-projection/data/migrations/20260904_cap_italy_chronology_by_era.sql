-- Keep three chronological levels for ancient and medieval Italy, while
-- modern Italy remains capped at two. Regional entries remain direct leaves.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE italy_period_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO italy_period_root VALUES
('unified:20b71263-7e5d-5453-b845-dd969aa0088c'),
('unified:04f6bb08-f9e4-5be5-b988-754494c68bad'),
('unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6'),
('unified:d650d0ef-c555-5d70-8cd3-71e373eaa11c'),
('bokheim:history-lcc:ed5e6e57-21f0-5e47-ae92-37942c28b07a');

CREATE TEMP TABLE retained_italy_period(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO retained_italy_period VALUES
-- Ancient leaves.
('bokheim:history-lcc-full:b653a5c0-64a0-5b59-9315-4659785bb4f4'),
('unified:d7da47b6-0b98-59cf-981c-d9a881b4e840'),
('unified:90c811cc-c7f5-59dd-9753-65cd34bfccfb'),
-- Medieval leaves.
('unified:a200ed65-6b67-5c7b-923a-9b6381a8cc6c'),
('unified:adaff4b6-0109-5c85-92d7-e833456d886e'),
-- Ancient and medieval third-level leaves.
('bokheim:history-lcc-full:383d5699-4cf9-503c-9369-d0128dff5b9b'),
('bokheim:history-lcc:ab76a4c5-3ab6-584c-8b8f-ef18f27e9793'),
('bokheim:history-lcc:c0f07f47-c555-56da-80a1-da2aec6047d7'),
('bokheim:history-lcc:2cd340a4-b388-5de6-9c3b-c83c8d48fc4e'),
('bokheim:history-lcc:65c8afe8-6c1c-5e64-a87a-e5d2b0281ca3'),
('bokheim:history-lcc:dda3fe72-b5da-5ce6-8024-e749ab1b1506'),
('bokheim:history-lcc-full:f9d9aa2c-4d8c-55a6-b691-7f742b0628e3'),
('bokheim:history-lcc:a50c453d-2b25-51fc-97b8-55e25684f845'),
('bokheim:history-lcc:de19681a-be8b-5235-9a7f-b344a5f911a3'),
('bokheim:history-lcc:011e61a5-ba76-5670-851e-c5886083216e'),
('bokheim:history-lcc:9cf18e8c-dcf0-5459-be5e-51bfe66f31ab'),
-- Modern leaves promoted from the overlapping general schedule.
('bokheim:lcc-detail:84976762-9d9e-5671-ae98-3b353f5983a9'),
('bokheim:lcc-detail:cf33d0ca-b1a0-5d56-b304-bca6e57a85a4'),
('bokheim:lcc-detail:cbda8f46-628e-58fe-bf57-350b230f2216'),
('bokheim:lcc-detail:07af9948-c0cf-5107-abf3-c6d8725324aa'),
('bokheim:lcc-detail:c72ccdbe-0c0c-560c-9cf9-e5dafb9a5672'),
('bokheim:lcc-detail:7f265473-8d9f-55b3-a482-8d21b3123517'),
('bokheim:lcc-detail:939b5b17-5243-5ffe-b781-7133ced33e73'),
('bokheim:lcc-detail:c0cfb1e2-876b-538c-b026-0fcd7cdc9a0d'),
('bokheim:lcc-detail:89001e57-5d0a-5e23-9df9-d7fc89fe0ef7');

-- Record where selectors from removed detail should continue to resolve.
CREATE TEMP TABLE selector_target(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;

WITH RECURSIVE mapped(source_id,target_id) AS (
    VALUES
    ('bokheim:history-lcc-full:b653a5c0-64a0-5b59-9315-4659785bb4f4','bokheim:history-lcc-full:b653a5c0-64a0-5b59-9315-4659785bb4f4'),
    ('unified:d7da47b6-0b98-59cf-981c-d9a881b4e840','unified:d7da47b6-0b98-59cf-981c-d9a881b4e840'),
    ('bokheim:history-lcc:941ba24a-688a-5680-b439-b057ab52fa18','unified:d7da47b6-0b98-59cf-981c-d9a881b4e840'),
    ('unified:90c811cc-c7f5-59dd-9753-65cd34bfccfb','unified:90c811cc-c7f5-59dd-9753-65cd34bfccfb'),
    ('unified:a200ed65-6b67-5c7b-923a-9b6381a8cc6c','unified:a200ed65-6b67-5c7b-923a-9b6381a8cc6c'),
    ('unified:adaff4b6-0109-5c85-92d7-e833456d886e','unified:adaff4b6-0109-5c85-92d7-e833456d886e'),
    ('bokheim:history-lcc-full:383d5699-4cf9-503c-9369-d0128dff5b9b','bokheim:history-lcc-full:383d5699-4cf9-503c-9369-d0128dff5b9b'),
    ('bokheim:history-lcc:ab76a4c5-3ab6-584c-8b8f-ef18f27e9793','bokheim:history-lcc:ab76a4c5-3ab6-584c-8b8f-ef18f27e9793'),
    ('bokheim:history-lcc:c0f07f47-c555-56da-80a1-da2aec6047d7','bokheim:history-lcc:c0f07f47-c555-56da-80a1-da2aec6047d7'),
    ('bokheim:history-lcc:2cd340a4-b388-5de6-9c3b-c83c8d48fc4e','bokheim:history-lcc:2cd340a4-b388-5de6-9c3b-c83c8d48fc4e'),
    ('bokheim:history-lcc:65c8afe8-6c1c-5e64-a87a-e5d2b0281ca3','bokheim:history-lcc:65c8afe8-6c1c-5e64-a87a-e5d2b0281ca3'),
    ('bokheim:history-lcc:dda3fe72-b5da-5ce6-8024-e749ab1b1506','bokheim:history-lcc:dda3fe72-b5da-5ce6-8024-e749ab1b1506'),
    ('bokheim:history-lcc-full:f9d9aa2c-4d8c-55a6-b691-7f742b0628e3','bokheim:history-lcc-full:f9d9aa2c-4d8c-55a6-b691-7f742b0628e3'),
    ('bokheim:history-lcc:a50c453d-2b25-51fc-97b8-55e25684f845','bokheim:history-lcc:a50c453d-2b25-51fc-97b8-55e25684f845'),
    ('bokheim:history-lcc:de19681a-be8b-5235-9a7f-b344a5f911a3','bokheim:history-lcc:de19681a-be8b-5235-9a7f-b344a5f911a3'),
    ('bokheim:history-lcc:011e61a5-ba76-5670-851e-c5886083216e','bokheim:history-lcc:011e61a5-ba76-5670-851e-c5886083216e'),
    ('bokheim:history-lcc:9cf18e8c-dcf0-5459-be5e-51bfe66f31ab','bokheim:history-lcc:9cf18e8c-dcf0-5459-be5e-51bfe66f31ab'),
    ('unified:c679d512-7e51-5a5b-933b-871de9c6af62','unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6'),
    ('unified:b732cc8a-a11c-5987-be28-5a6e7af36e88','unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6'),
    ('unified:462d9603-c343-507e-9a09-00cb6abb1888','unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6'),
    ('bokheim:history-lcc:ed5e6e57-21f0-5e47-ae92-37942c28b07a','unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6'),
    ('bokheim:lcc-detail:fd0328f1-543d-542d-b6a6-5f5c058486f6','unified:04f6bb08-f9e4-5be5-b988-754494c68bad')
    UNION
    SELECT child.concept_id,mapped.target_id
    FROM mapped
    JOIN concept_parent child ON child.parent_concept_id=mapped.source_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_italy_period)
)
INSERT OR IGNORE INTO selector_target SELECT source_id,target_id FROM mapped;

-- The combined general wrapper spans medieval and modern history, so retain
-- its broad selectors on Italy itself.
INSERT OR IGNORE INTO selector_target
VALUES ('unified:d650d0ef-c555-5d70-8cd3-71e373eaa11c','unified:97e56aeb-5265-540c-9edc-989bb0bcc66e');

CREATE TEMP TABLE selector_copy AS
SELECT DISTINCT target.target_id,source.system_id,source.selector
FROM selector_target target
JOIN source_selector source ON source.concept_id=target.source_id
WHERE target.source_id<>target.target_id
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id=target.target_id
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT copy.target_id,copy.system_id,copy.selector,
       COALESCE((SELECT max(existing.ordinal)+1
                 FROM source_selector existing
                 WHERE existing.concept_id=copy.target_id
                   AND existing.system_id=copy.system_id),0)
       + row_number() OVER (PARTITION BY copy.target_id,copy.system_id ORDER BY copy.selector)-1
FROM selector_copy copy;

-- Promote the nine modern date ranges before removing their old wrapper.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT period.concept_id,'unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6',
       COALESCE((SELECT max(existing.ordinal)+1 FROM concept_parent existing WHERE existing.concept_id=period.concept_id),0),
       old_edge.display_order
FROM retained_italy_period period
JOIN concept_parent old_edge ON old_edge.concept_id=period.concept_id
WHERE old_edge.parent_concept_id='unified:d650d0ef-c555-5d70-8cd3-71e373eaa11c'
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id=period.concept_id
        AND existing.parent_concept_id='unified:93d9c99f-04be-5f7c-9a4c-abff0ecfbcd6'
  );

CREATE TEMP TABLE italy_period_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM italy_period_root root
    JOIN concept_parent child ON child.parent_concept_id=root.concept_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_italy_period)
    UNION
    SELECT child.concept_id
    FROM retained_italy_period retained
    JOIN concept_parent child ON child.parent_concept_id=retained.concept_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_italy_period)
    UNION
    SELECT child.concept_id
    FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id NOT IN (SELECT concept_id FROM retained_italy_period)
)
INSERT INTO italy_period_detail
SELECT concept_id FROM detail
UNION SELECT concept_id FROM italy_period_root
WHERE concept_id IN (
    'unified:d650d0ef-c555-5d70-8cd3-71e373eaa11c',
    'bokheim:history-lcc:ed5e6e57-21f0-5e47-ae92-37942c28b07a'
);

CREATE TEMP TABLE terminal_italy_period(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO terminal_italy_period
SELECT concept_id FROM retained_italy_period
WHERE concept_id NOT IN (
    'bokheim:history-lcc-full:b653a5c0-64a0-5b59-9315-4659785bb4f4',
    'unified:d7da47b6-0b98-59cf-981c-d9a881b4e840',
    'unified:90c811cc-c7f5-59dd-9753-65cd34bfccfb',
    'unified:a200ed65-6b67-5c7b-923a-9b6381a8cc6c',
    'unified:adaff4b6-0109-5c85-92d7-e833456d886e'
);

-- Disconnect every terminal chronological leaf from deeper detail.
DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM terminal_italy_period);

-- Remove the overlapping roots from Italy.
DELETE FROM concept_parent
WHERE parent_concept_id='unified:97e56aeb-5265-540c-9edc-989bb0bcc66e'
  AND concept_id IN (
      'unified:d650d0ef-c555-5d70-8cd3-71e373eaa11c',
      'bokheim:history-lcc:ed5e6e57-21f0-5e47-ae92-37942c28b07a'
  );

-- Remove only concepts that are not retained through another curated route.
CREATE TEMP TABLE shared_italy_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE shared(concept_id) AS (
    SELECT detail.concept_id
    FROM italy_period_detail detail
    JOIN concept_parent incoming ON incoming.concept_id=detail.concept_id
    WHERE incoming.parent_concept_id NOT IN (SELECT concept_id FROM italy_period_detail)
      AND incoming.parent_concept_id NOT IN (SELECT concept_id FROM italy_period_root)
      AND incoming.parent_concept_id NOT IN (SELECT concept_id FROM retained_italy_period)
    UNION
    SELECT child.concept_id
    FROM shared parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    WHERE child.concept_id IN (SELECT concept_id FROM italy_period_detail)
)
INSERT INTO shared_italy_detail SELECT concept_id FROM shared;

DELETE FROM concept_parent
WHERE concept_id IN (
    SELECT concept_id FROM italy_period_detail
    WHERE concept_id NOT IN (SELECT concept_id FROM shared_italy_detail)
)
OR parent_concept_id IN (
    SELECT concept_id FROM italy_period_detail
    WHERE concept_id NOT IN (SELECT concept_id FROM shared_italy_detail)
);
DELETE FROM concept
WHERE concept_id IN (
    SELECT concept_id FROM italy_period_detail
    WHERE concept_id NOT IN (SELECT concept_id FROM shared_italy_detail)
);

UPDATE concept SET preferred_label='Roman Kingdom & Republic'
WHERE concept_id='unified:d7da47b6-0b98-59cf-981c-d9a881b4e840';
UPDATE concept SET preferred_label='Roman Empire'
WHERE concept_id='unified:90c811cc-c7f5-59dd-9753-65cd34bfccfb';
UPDATE concept SET preferred_label='Early Medieval, 476–1268'
WHERE concept_id='unified:a200ed65-6b67-5c7b-923a-9b6381a8cc6c';
UPDATE concept SET preferred_label='Late Medieval, 1268–1492'
WHERE concept_id='unified:adaff4b6-0109-5c85-92d7-e833456d886e';
UPDATE concept SET preferred_label='1975–Present'
WHERE concept_id='bokheim:lcc-detail:89001e57-5d0a-5e23-9df9-d7fc89fe0ef7';

DROP TABLE shared_italy_detail;
DROP TABLE italy_period_detail;
DROP TABLE terminal_italy_period;
DROP TABLE selector_copy;
DROP TABLE selector_target;
DROP TABLE retained_italy_period;
DROP TABLE italy_period_root;
COMMIT;
