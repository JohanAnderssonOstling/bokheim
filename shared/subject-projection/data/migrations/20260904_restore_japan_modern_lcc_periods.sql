-- Repair Japan's modern chronology omitted when the LCC DS schedule crossed
-- the page 160/161 boundary. Subjects and ranges follow the official schedule.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO concept(concept_id,preferred_label) VALUES
('bokheim:lcc-repair:japan:meiji','Meiji, 1868–1912'),
('bokheim:lcc-repair:japan:taisho','Taisho, 1912–1926'),
('bokheim:lcc-repair:japan:early-showa','Early Showa, 1926–1945'),
('bokheim:lcc-repair:japan:postwar-showa','Postwar Showa, 1945–1989'),
('bokheim:lcc-repair:japan:heisei','Heisei, 1989–Present');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
('bokheim:lcc-repair:japan:meiji','unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d',0,40),
('bokheim:lcc-repair:japan:taisho','unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d',0,50),
('bokheim:lcc-repair:japan:early-showa','unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d',0,60),
('bokheim:lcc-repair:japan:postwar-showa','unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d',0,70),
('bokheim:lcc-repair:japan:heisei','unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d',0,80);

UPDATE concept_parent SET display_order=30
WHERE concept_id='bokheim:history-lcc-full:7da2d8b5-dde0-58b9-a3b6-80fa28e23534'
  AND parent_concept_id='unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d';
UPDATE concept_parent SET display_order=40 WHERE concept_id='bokheim:lcc-repair:japan:meiji';
UPDATE concept_parent SET display_order=50 WHERE concept_id='bokheim:lcc-repair:japan:taisho';
UPDATE concept_parent SET display_order=60 WHERE concept_id='bokheim:lcc-repair:japan:early-showa';
UPDATE concept_parent SET display_order=70 WHERE concept_id='bokheim:lcc-repair:japan:postwar-showa';
UPDATE concept_parent SET display_order=80 WHERE concept_id='bokheim:lcc-repair:japan:heisei';

INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('bokheim:lcc-repair:japan:meiji','lcc','DS881.98..DS884.Z@6',0),
('bokheim:lcc-repair:japan:taisho','lcc','DS885.8..DS888@7',0),
('bokheim:lcc-repair:japan:early-showa','lcc','DS888.15..DS888.5@7',0),
('bokheim:lcc-repair:japan:postwar-showa','lcc','DS888.84..DS890.3@7',0),
('bokheim:lcc-repair:japan:heisei','lcc','DS891..DS891.5@7',0);

-- Official aggregate classes remain fallback mappings on Japan, not browse nodes.
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
('unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d','lcc','DS881.85..DS891.5@5',155),
('unified:009fc8d9-5d02-5c3a-ad9b-d8b84a6b070d','lcc','DS884.5..DS890.3@6',156);

COMMIT;
