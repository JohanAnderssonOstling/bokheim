-- Restore the six useful master chronology concepts directly beneath the
-- unified Egypt concept. Period nodes are terminal in the curated browser.

PRAGMA foreign_keys = ON;
ATTACH DATABASE 'master-taxonomy-v2.sqlite3' AS master;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'Egypt to 640'
WHERE concept_id = 'unified:34b1b731-0f45-5780-9f18-8bcf2e7faf70';
UPDATE concept SET preferred_label = '638–868'
WHERE concept_id = 'bokheim:history-lcc-full:4c443c89-179b-569c-916d-cce36151f786';
UPDATE concept SET preferred_label = '1250–1517'
WHERE concept_id = 'bokheim:history-lcc-full:d0b476e9-1622-5776-b6c5-9bd237b449ca';
UPDATE concept SET preferred_label = '1798–1879'
WHERE concept_id = 'bokheim:history-lcc-full:0a43fb53-d6bc-5828-b87b-8b2b68fecf2b';
UPDATE concept SET preferred_label = '1879–1952'
WHERE concept_id = 'bokheim:history-lcc-full:80c3d160-1d4f-5df7-aa78-c3e28d065fb7';
UPDATE concept SET preferred_label = '1952–'
WHERE concept_id = 'bokheim:history-lcc-full:00329dd3-bd19-5668-8be9-c68031c9558e';

CREATE TEMP TABLE egypt_period(concept_id TEXT PRIMARY KEY, display_order INTEGER NOT NULL) WITHOUT ROWID;
INSERT INTO egypt_period VALUES
 ('unified:34b1b731-0f45-5780-9f18-8bcf2e7faf70', 0),
 ('bokheim:history-lcc-full:4c443c89-179b-569c-916d-cce36151f786', 10),
 ('bokheim:history-lcc-full:d0b476e9-1622-5776-b6c5-9bd237b449ca', 20),
 ('bokheim:history-lcc-full:0a43fb53-d6bc-5828-b87b-8b2b68fecf2b', 30),
 ('bokheim:history-lcc-full:80c3d160-1d4f-5df7-aa78-c3e28d065fb7', 40),
 ('bokheim:history-lcc-full:00329dd3-bd19-5668-8be9-c68031c9558e', 50);

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id,
       'unified:b1da6c49-e4e6-5dca-b82a-8f37a1793e80',
       COALESCE((SELECT max(existing.ordinal) + 1 FROM concept_parent existing
                 WHERE existing.concept_id = period.concept_id), 0),
       period.display_order
FROM egypt_period period;

DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM egypt_period);

CREATE TEMP TABLE valid_root(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO valid_root
SELECT concept.concept_id
FROM concept
WHERE NOT EXISTS (SELECT 1 FROM concept_parent WHERE concept_parent.concept_id=concept.concept_id)
  AND NOT EXISTS (SELECT 1 FROM master.concept_parent WHERE master.concept_parent.concept_id=concept.concept_id);

CREATE TEMP TABLE reachable(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE retained(concept_id) AS (
 SELECT concept_id FROM valid_root
 UNION
 SELECT edge.concept_id FROM retained
 JOIN concept_parent edge ON edge.parent_concept_id=retained.concept_id
)
INSERT INTO reachable SELECT concept_id FROM retained;

DELETE FROM concept_parent
WHERE concept_id NOT IN (SELECT concept_id FROM reachable)
   OR parent_concept_id NOT IN (SELECT concept_id FROM reachable);
DELETE FROM concept WHERE concept_id NOT IN (SELECT concept_id FROM reachable);

DROP TABLE reachable;
DROP TABLE valid_root;
DROP TABLE egypt_period;
COMMIT;
DETACH DATABASE master;
