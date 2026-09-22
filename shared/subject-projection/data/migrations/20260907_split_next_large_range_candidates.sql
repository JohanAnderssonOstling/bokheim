-- Split the next high-hit range owners along official LCC divisions. Broad
-- parent ranges remain only as fallbacks for genuinely broad source values and
-- uncovered schedule gaps.

-- Applied sciences and technology subclasses.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('T','T',5563), ('TA','TA',5564), ('TC','TH',5564),
  ('TJ','TJ',5565), ('TK','TK',5566), ('TL','TL',458),
  ('TP','TP',5567), ('TS','TS',361),
  ('V','VM',53070);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
  (5564,'TC'), (5565,'TJ'), (5566,'TK'), (458,'TL'),
  (5567,'TP'), (361,'TS'), (53070,'V-VM'),
  (458,'TL787-4050.22');

-- Mathematics major divisions.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('QA1','QA99',5722),
  ('QA101','QA145',5716),
  ('QA150','QA272',5717),
  ('QA273','QA280',4961),
  ('QA299.6','QA433',5719),
  ('QA440','QA610.9',4939),
  ('QA611','QA614.97',4971),
  ('QA615','QA699',4939),
  ('QA801','QA939',5721);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
  (5717,'QA150-272'), (5717,'QA164-167.2'), (5717,'QA221-224'),
  (5719,'QA299.6-433'), (5719,'QA402-402.37'),
  (4939,'QA440-699'), (4971,'QA611-614.97'),
  (5721,'QA801-939');

-- Philosophy's B schedule is history/traditions after the reference section;
-- the former B1-B5802 ownership by "Philosophy Reference" was misleading.
UPDATE lcc_range SET concept_id=4487
WHERE concept_id=5532 AND start_code='B1' AND end_code='B5802';
UPDATE lcc_selector SET concept_id=4487
WHERE concept_id=5532 AND selector='B1-5802';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('B1','B99',5532),
  ('B108','B5799.9',4507);

-- Architecture top-level divisions and chronological history.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('NA1','NA63',5704), ('NA100','NA150',5698),
  ('NA190','NA1555.5',574), ('NA2000','NA2460',5704),
  ('NA2500','NA4050',5698), ('NA4100','NA8480',5699),
  ('NA9000','NA9428',5701),
  ('NA205','NA207',580), ('NA210','NA340',575),
  ('NA350','NA489',578), ('NA510','NA512',581),
  ('NA685','NA689',577);

-- Family and life-stage schedule gaps with established children.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('HQ755.7','HQ759.92',11804),
  ('HQ767.8','HQ792.2',12281),
  ('HQ793','HQ799.2',12282),
  ('HQ1059.4','HQ1064',12294);

-- Marketing and advertising subdivisions.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('HF5410','HF5415.1259',12801),
  ('HF5415.2','HF5415.34',12805),
  ('HF5415.126','HF5415.1262',12802),
  ('HF5415.126','HF5415.1264',12803),
  ('HF5415.1265','HF5415.1269',12806);

-- Ball games: official sport blocks.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53393,'Baseball'), (53394,'Basketball'), (53395,'Billiards & Pool'),
  (53396,'Bowling'), (53397,'Cricket'), (53398,'Croquet'),
  (53399,'Football'), (53400,'Golf'), (53401,'Lacrosse'),
  (53402,'Tennis & Racket Sports'), (53403,'Polo'), (53404,'Volleyball');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53393,53212,0),(53394,53212,0),(53395,53212,0),(53396,53212,0),
  (53397,53212,0),(53398,53212,0),(53399,53212,0),(53400,53212,0),
  (53401,53212,0),(53402,53212,0),(53403,53212,0),(53404,53212,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('GV862','GV881.6',53393), ('GV882','GV889.3',53394),
  ('GV891','GV900',53395), ('GV901','GV910.5',53396),
  ('GV911','GV929.3',53397), ('GV931','GV935',53398),
  ('GV937','GV960',53399), ('GV961','GV988',53400),
  ('GV989','GV989.2',53401), ('GV990','GV1008',53402),
  ('GV1010','GV1011.6',53403), ('GV1015','GV1015.57',53404);

-- Mammal divisions with substantial use.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53405,'Mammal Geography'), (53406,'Carnivores'), (53407,'Cetaceans'),
  (53408,'Bats'), (53409,'Marsupials'), (53410,'Primates'),
  (53411,'Rodents'), (53412,'Ungulates'),
  (53413,'Mammal Physiology'), (53414,'Mammal Behavior');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53405,8709,0),(53406,8709,0),(53407,8709,0),(53408,8709,0),
  (53409,8709,0),(53410,8709,0),(53411,8709,0),(53412,8709,0),
  (53413,8709,0),(53414,8709,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('QL708.7','QL736.5',53405), ('QL737.C2','QL737.C28',53406),
  ('QL737.C4','QL737.C438',53407), ('QL737.C5','QL737.C595',53408),
  ('QL737.M3','QL737.M476',53409), ('QL737.P9','QL737.P968',53410),
  ('QL737.R6','QL737.R688',53411), ('QL737.U4','QL737.U64',53412),
  ('QL739.2','QL739.23',53413), ('QL739.3','QL739.5',53414);

-- Composer biographies.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53415,'Collective Composer Biographies'),
  (53416,'Individual Composer Biographies');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53415,12476,0),(53416,12476,0);
UPDATE lcc_range SET concept_id=53415
WHERE concept_id=12476 AND start_code='ML390' AND end_code='ML390';
UPDATE lcc_range SET concept_id=53416
WHERE concept_id=12476 AND start_code='ML410' AND end_code='ML410';

-- Jewish diaspora topics.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53417,'Jewish Communities by Region'), (53418,'Jewish Identity'),
  (53419,'Antisemitism'), (53420,'Zionism');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53417,50840,0),(53418,50840,0),(53419,50840,0),(53420,50840,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('DS135','DS142.9',53417), ('DS143','DS144.9',53418),
  ('DS145','DS148.9',53419), ('DS149','DS151',53420);
