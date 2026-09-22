-- Restore the broad coded Portuguese periods that follow the Iberian Union in
-- the LCC D-DR schedule. Keep the curated country tree shallow: the individual
-- monarchs and events below these ranges remain available only in the master.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id, preferred_label) VALUES
  ('bokheim:history-lcc:425166ca-ed73-57e8-8f10-ff0cc521c34f', 'House of Braganza, 1640–1816'),
  ('bokheim:history-lcc:41748453-985e-5dc5-bcce-06f40669afa1', 'Portuguese Monarchy, 1816–1908'),
  ('bokheim:history-lcc:a887861d-f17b-5554-be81-22838a2865a8', 'Portugal, 20th Century');

UPDATE concept SET preferred_label='House of Braganza, 1640–1816'
WHERE concept_id='bokheim:history-lcc:425166ca-ed73-57e8-8f10-ff0cc521c34f';
UPDATE concept SET preferred_label='Portuguese Monarchy, 1816–1908'
WHERE concept_id='bokheim:history-lcc:41748453-985e-5dc5-bcce-06f40669afa1';
UPDATE concept SET preferred_label='Portugal, 20th Century'
WHERE concept_id='bokheim:history-lcc:a887861d-f17b-5554-be81-22838a2865a8';

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
  ('bokheim:history-lcc:425166ca-ed73-57e8-8f10-ff0cc521c34f','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',0,45),
  ('bokheim:history-lcc:41748453-985e-5dc5-bcce-06f40669afa1','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',0,50),
  ('bokheim:history-lcc:a887861d-f17b-5554-be81-22838a2865a8','unified:bcf05a2d-7986-5262-9c58-af0dcea68f64',0,55);

DELETE FROM source_selector WHERE concept_id='bokheim:history-lcc:425166ca-ed73-57e8-8f10-ff0cc521c34f' AND system_id='lcc' AND selector='DP632..DP632@6';
DELETE FROM source_selector WHERE concept_id='bokheim:history-lcc:41748453-985e-5dc5-bcce-06f40669afa1' AND system_id='lcc' AND selector='DP645..DP660@6';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
  ('bokheim:history-lcc:425166ca-ed73-57e8-8f10-ff0cc521c34f','lcc','DP632..DP644.9@6',0),
  ('bokheim:history-lcc:41748453-985e-5dc5-bcce-06f40669afa1','lcc','DP645..DP669@6',0),
  ('bokheim:history-lcc:a887861d-f17b-5554-be81-22838a2865a8','lcc','DP670..DP682.2@6',0);

-- These precise selectors may previously have been retained on Portugal as
-- fallbacks. The period leaves are now their more useful destinations.
DELETE FROM source_selector
WHERE concept_id='unified:bcf05a2d-7986-5262-9c58-af0dcea68f64'
  AND system_id='lcc'
  AND selector IN ('DP632..DP632@6','DP632..DP644.9@6',
                   'DP645..DP660@6','DP645..DP669@6','DP670..DP682.2@6');

COMMIT;
