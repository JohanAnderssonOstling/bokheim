-- Remove generic topical entries from European regional history roots.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE topic_move(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO topic_move VALUES
('bokheim:history-lcc:1cb9ba67-b5b7-520e-9218-9f55eaaa0df3','unified:68284cc9-f9d0-5474-a92c-caabd008b66e'),
('bokheim:history-lcc:6dad4037-4fba-5044-8320-e4d382294747','unified:68284cc9-f9d0-5474-a92c-caabd008b66e'),
('bokheim:history-lcc-full:753dd543-4bae-52eb-8e93-5405260db2e4','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'),
('bokheim:lcc-detail:af8d7a2e-e0bb-57c3-a069-0bb143ff72ef','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'),
('bokheim:lcc-detail:7db4abf3-6453-5f53-bc4c-067e50d1dcb2','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'),
('bokheim:lcc-detail:0f99d737-1201-5270-8325-7036dc103840','unified:49ac1f3c-ff3b-5d4a-b864-83cd3bf050ec'),
('bokheim:history-lcc-full:45a750b2-e168-5a13-8b57-2ed1f864b620','unified:d6daa986-0fa4-5394-945e-614d7b09d3ca'),
('bokheim:history-lcc-full:82e795b6-fd69-512d-a913-127939f3f988','unified:d6daa986-0fa4-5394-945e-614d7b09d3ca'),
('bokheim:history-lcc-full:2d8f2d55-a9cf-5857-a87c-b1f5f833342a','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad'),
('bokheim:history-lcc-full:42755d18-78d4-59d4-b28d-aba4864ee7b8','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad'),
('bokheim:history-lcc-full:58d6edd2-e9a3-597c-8654-4b91fb49338b','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad');
WITH candidates AS (
 SELECT m.source_id,m.target_id,s.system_id,s.selector,row_number() OVER(PARTITION BY m.target_id,s.system_id ORDER BY s.selector)-1 rn
 FROM topic_move m JOIN source_selector s ON s.concept_id=m.source_id
 WHERE NOT EXISTS (SELECT 1 FROM source_selector x WHERE x.concept_id=m.target_id AND x.system_id=s.system_id AND x.selector=s.selector)
)
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,COALESCE((SELECT max(x.ordinal)+1 FROM source_selector x WHERE x.concept_id=candidates.target_id AND x.system_id=candidates.system_id),0)+rn FROM candidates;
DELETE FROM concept_parent WHERE concept_id='bokheim:history-lcc-full:3d78cc5b-c19f-54fa-93e9-27d5de2ecb82';
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES('bokheim:history-lcc-full:3d78cc5b-c19f-54fa-93e9-27d5de2ecb82','unified:2a2ad62e-3583-58f2-a61e-aa4f66982aad',0,90);
DELETE FROM concept_parent WHERE concept_id IN (SELECT source_id FROM topic_move) OR parent_concept_id IN (SELECT source_id FROM topic_move);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM topic_move) AND NOT EXISTS (SELECT 1 FROM concept_parent p WHERE p.concept_id=concept.concept_id);
DROP TABLE topic_move;
COMMIT;
