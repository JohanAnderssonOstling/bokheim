-- Individual languages are terminal concepts in the curated taxonomy. Keep
-- language families expandable, but remove periods, dialects, grammar,
-- dictionaries, literature, and other catalog detail below a language.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

CREATE TEMP TABLE individual_language(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO individual_language VALUES
 ('unified:3f8c5cc2-5747-5108-9e2d-a370e6de9348'), -- Arabic
 ('unified:d2ad3cff-ffe9-5477-b90c-daf89a24d1ea'), -- Aramaic
 ('unified:92ca76da-7ecb-5316-b102-1cac007801b6'), -- Armenian
 ('unified:6ab90aa3-4009-5add-ba73-7011a96fd8e1'), -- Akkadian
 ('unified:53162bf3-064c-5869-b952-4d8131442ea4'), -- Catalan
 ('unified:3aa9ac85-654d-5303-be7b-4d285775a8aa'), -- Coptic
 ('unified:ef46082d-9488-5de7-ab40-e696c601d34a'), -- Dutch
 ('unified:d2e55b6b-d533-526e-bdf8-d338a4a99a69'), -- English
 ('unified:a7694194-aab5-554e-a4d7-4de98cdd2725'), -- Flemish
 ('unified:26be5fa1-ae0d-513c-aaaa-b06a318d187e'), -- French
 ('unified:278c4e55-de8b-510e-91eb-6934e4c5d86e'), -- German
 ('unified:6188f929-779b-51ef-b624-452f41055380'), -- Hebrew
 ('unified:0f564de7-87b8-500e-b85b-753c9e28f17b'), -- Italian
 ('unified:b17623d3-cbfd-5a12-8f5d-97aa904f3563'), -- Latin
 ('unified:60d174ed-64eb-5c4e-8f24-05bace1d357b'), -- Portuguese
 ('unified:ea1b541a-32a0-56ac-8114-c906e13d2b44'), -- Romanian
 ('unified:ec2a0772-9c9f-58fe-8b81-cb60fe0d3594'), -- Sardinian
 ('unified:a152dbcc-1329-5b23-ba1e-5ca0565b918d'), -- Spanish
 ('unified:d2c507b6-3387-5da7-a3d3-a2e470d63e39'), -- Sumerian
 ('unified:908d09a0-9542-5aa4-80d8-068fb3736570'); -- Syriac

DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM individual_language);

-- Delete detail disconnected by the leaf rule, while retaining concepts that
-- remain reachable through another curated subject branch.
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
DROP TABLE individual_language;
COMMIT;
DETACH DATABASE master;
