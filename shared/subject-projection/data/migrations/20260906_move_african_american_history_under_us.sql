-- Move the existing African American history subject under United States.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
UPDATE concept_parent SET parent_concept_id='unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a', display_order=(SELECT COALESCE(MAX(display_order)+1,0) FROM concept_parent WHERE parent_concept_id='unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a') WHERE concept_id='unified:62212ffe-4748-5f7e-b5c0-1091a13f860a' AND parent_concept_id='unified:386d4de9-6fb1-5098-8567-95f9470142e4';
COMMIT;
