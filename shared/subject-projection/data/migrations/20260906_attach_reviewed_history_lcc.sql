-- Attach seven user-reviewed history, social-history and biography LCC codes.
-- No new concepts, parent edges, permissive ranges or wildcards. Existing matches were preserved.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:62212ffe-4748-5f7e-b5c0-1091a13f860a','lcc','E185',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:62212ffe-4748-5f7e-b5c0-1091a13f860a' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:62212ffe-4748-5f7e-b5c0-1091a13f860a','lcc','E185.86',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:62212ffe-4748-5f7e-b5c0-1091a13f860a' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a','lcc','E178.1',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'unified:33fe9701-b7fc-5d88-8044-f079d3b60131','lcc','E176.1',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='unified:33fe9701-b7fc-5d88-8044-f079d3b60131' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:social-history:united-states','lcc','E169.1',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:social-history:united-states' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:social-history:united-states','lcc','E169.12',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:social-history:united-states' AND system_id='lcc';
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) SELECT 'bokheim:social-history:japan','lcc','DS821',COALESCE(MAX(ordinal)+1,0) FROM source_selector WHERE concept_id='bokheim:social-history:japan' AND system_id='lcc';
COMMIT;
