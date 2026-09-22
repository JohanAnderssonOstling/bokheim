-- Remove the fifteen longest single-code LCC detail labels. Preserve each
-- exact selector on its retained parent before deleting the leaf concept.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE long_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO long_detail VALUES
 ('bokheim:lcc-detail:1951db22-ee37-5fa4-8b6c-6fab63f6b79c'),
 ('bokheim:lcc-detail:08f00646-6d0e-5188-932a-6fdd1e88c071'),
 ('bokheim:lcc-detail:e42d1337-16d3-5218-be73-e90c1da15991'),
 ('bokheim:lcc-detail:25cc440a-6baf-5289-9886-8fbeffc82ec4'),
 ('bokheim:history-lcc-full:02ac7177-3b6a-5df8-9400-6b603cc2b00b'),
 ('bokheim:lcc-detail:3cbdc8b8-c4a3-5aee-8c75-ab8b459d9f12'),
 ('bokheim:lcc-detail:61247679-df63-5351-9fb0-9792668ed393'),
 ('bokheim:lcc-detail:ce1456ae-e3a1-50c3-909b-b3b3a1bc70d9'),
 ('bokheim:lcc-detail:c8aded4e-1c1a-5c4e-800b-e182dcd6e883'),
 ('bokheim:lcc-detail:6ea4b05b-d4c0-5684-b173-e6789f15602f'),
 ('bokheim:lcc-detail:b4684045-f503-5f72-a6ae-f7d6c2e7fd01'),
 ('bokheim:lcc-detail:44b0106e-ae72-5bf1-b628-98b9fb3749c9'),
 ('bokheim:lcc-detail:e55dcf4f-8983-5c6c-899d-698c932051c4'),
 ('bokheim:lcc-detail:44f4ec8d-ae5e-544c-9aee-c3bc78d4c0d8'),
 ('bokheim:lcc-detail:deed514d-6aa4-5ded-9f7b-f1feb02341e4');

CREATE TEMP TABLE transferred(
 parent_id TEXT NOT NULL,
 system_id TEXT NOT NULL,
 selector TEXT NOT NULL,
 ordinal INTEGER NOT NULL,
 PRIMARY KEY(parent_id, system_id, selector)
) WITHOUT ROWID;

INSERT INTO transferred
SELECT cp.parent_concept_id,
       selector.system_id,
       selector.selector,
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM source_selector existing
                 WHERE existing.concept_id=cp.parent_concept_id
                   AND existing.system_id=selector.system_id), 0)
FROM long_detail detail
JOIN concept_parent cp ON cp.concept_id=detail.concept_id
JOIN source_selector selector ON selector.concept_id=detail.concept_id;

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT parent_id,system_id,selector,ordinal FROM transferred;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM long_detail);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM long_detail);

DROP TABLE transferred;
DROP TABLE long_detail;
COMMIT;
