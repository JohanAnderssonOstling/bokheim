-- Spain's curated children should be chronological periods only.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE spain_nonchronological(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO spain_nonchronological VALUES
 ('bokheim:lcc-detail:1ff8cfbe-a659-5eb8-8d6c-5061d0ce1c64'),
 ('bokheim:history-lcc-full:9705cde7-5652-5263-b69b-aed0d54b0d3a'),
 ('bokheim:history-lcc-full:b073e608-f669-5377-a357-90a33f5d4c78'),
 ('bokheim:history-lcc-full:5192082f-1f7e-5724-abea-a884e5c396cc'),
 ('bokheim:history-lcc-full:545dc0b4-0c5a-511b-b2d1-232c31a3db14');

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM spain_nonchronological);

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
DROP TABLE spain_nonchronological;
COMMIT;
DETACH DATABASE master;
