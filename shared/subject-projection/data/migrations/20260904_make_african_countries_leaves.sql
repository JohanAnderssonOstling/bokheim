-- African regions remain browseable, but individual countries are terminal.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE african_country(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO african_country VALUES
 ('unified:f52b29a0-8864-54b7-9460-3403aed7ba41'),
 ('unified:311fcd05-f87d-5d51-a6df-b4b2c2ba5e05'),
 ('unified:dd20ef8a-2f6f-58bd-acf2-832ecc022be2'),
 ('unified:6b79a347-32eb-56b1-a996-b6679b3863b7'),
 ('unified:c95d96a5-eb2a-53b1-8ba1-afe20b672854'),
 ('unified:1a4b5b46-1205-5aa3-ac46-62072c7663bc'),
 ('unified:df91e828-a780-53ee-b568-88f5e1ced94a'),
 ('unified:023b2c7b-7f50-545c-92ba-c12fb2a7cdda'),
 ('unified:70f2280a-67f3-5a8b-8af3-e239b5c489c4'),
 ('bokheim:history-lcc-full:86e7f545-c36e-5ed0-8a61-3cf7800b177e'),
 ('bokheim:history-lcc-full:e83f39da-73d3-5e19-87c7-0a5498b47aae'),
 ('bokheim:history-lcc-full:c4863d1b-2746-59d1-97a7-7e9ef93c9efb'),
 ('unified:5eb01a0a-ea26-5ba8-8a31-84d785c01058'),
 ('unified:3ce46fca-4b03-5cd3-81ad-229f425ef319'),
 ('unified:e3ad1bf3-e78a-5b6a-b5f3-32a22be71da3'),
 ('unified:4dde0914-c658-5618-8196-c6bb5f159e75'),
 ('unified:f04fe226-e7f6-5b9b-a15a-3a54214491c9'),
 ('unified:fdc3ab70-111b-5a30-b825-d8207f6a329a'),
 ('unified:c8bbd820-be93-5bd1-b173-e868678ac939'),
 ('bokheim:history-lcc-full:5401239a-dd7e-5686-a06b-0f0996d109f0'),
 ('unified:8101ec3f-509a-5f9a-b4a8-0279050b9d97'),
 ('unified:c0dd725f-e53d-51de-9008-5663bc0b14f3'),
 ('unified:e4aaae39-a38f-5e54-a44d-c78c624c0d92'),
 ('unified:adb0c9d0-cdf8-561e-a4ad-cad932f54ac1'),
 ('unified:7023e27f-5361-546b-b070-bc9660480634'),
 ('unified:276de897-0df2-5379-af08-f560bfc10476'),
 ('bokheim:history-lcc-full:c6594240-6447-5df2-9d99-7ca028c97d41'),
 ('unified:614064da-29bf-5c41-bc87-18f2218a0b20'),
 ('unified:3bd1ef93-676c-5e4b-97ae-92bb33ada2cd'),
 ('unified:98e6bf33-7bd0-553b-aeb3-097f8ad888f3'),
 ('unified:f889b064-11c3-5778-b753-20954919876a'),
 ('unified:4df06a56-4550-5207-8e92-a43acd1ca319'),
 ('unified:3a597bcf-cceb-589f-a7c3-a7457ab2ad0c'),
 ('unified:71066a24-afd6-5224-8a72-1bd3d9bc2fa9'),
 ('unified:9f947214-284f-513d-a106-30bbf03dd9c8'),
 ('unified:dde760e2-ddcb-5a67-819d-01c9b9b24cb3'),
 ('unified:a4bcb9b3-0237-5332-89bb-4c3ca70d0a99'),
 ('bokheim:history-lcc-full:03b83db5-9f93-5690-afac-7fcf84567020'),
 ('unified:0bcd58b9-034e-53a7-82b8-1ce5e872cdab'),
 ('unified:af68b079-0a66-5c6b-8eb6-ac58dfa7aede'),
 ('unified:269a0d85-4549-598d-aa34-53cf24eeb05e'),
 ('unified:a44d52b7-b5c5-5abf-a3e3-4aede0e6554e'),
 ('unified:ae9a93f5-2f01-5b34-8573-fcce92028715'),
 ('unified:1225ad11-53f9-5202-81cb-11a8c2ec5596'),
 ('unified:046085fd-1dd2-5ba1-b76e-5f6035925b85');

DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM african_country);

CREATE TEMP TABLE valid_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO valid_root
SELECT concept.concept_id
FROM concept
WHERE NOT EXISTS (
        SELECT 1 FROM concept_parent
        WHERE concept_parent.concept_id = concept.concept_id
    )
  AND NOT EXISTS (
        SELECT 1 FROM master.concept_parent
        WHERE master.concept_parent.concept_id = concept.concept_id
    );

CREATE TEMP TABLE reachable(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE retained(concept_id) AS (
    SELECT concept_id FROM valid_root
    UNION
    SELECT edge.concept_id
    FROM retained
    JOIN concept_parent AS edge
      ON edge.parent_concept_id = retained.concept_id
)
INSERT INTO reachable SELECT concept_id FROM retained;

DELETE FROM concept_parent
WHERE concept_id NOT IN (SELECT concept_id FROM reachable)
   OR parent_concept_id NOT IN (SELECT concept_id FROM reachable);
DELETE FROM concept
WHERE concept_id NOT IN (SELECT concept_id FROM reachable);

DROP TABLE reachable;
DROP TABLE valid_root;
DROP TABLE african_country;
COMMIT;
DETACH DATABASE master;
