-- Split the twenty reviewed broad-range subjects using official LCC divisions
-- and high-use exact notations. Whole-schedule source values remain at parents.

-- Art history by broad region.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53453,'Art History of the Americas'),(53454,'European Art History'),
 (53455,'Asian Art History'),(53456,'African Art History'),
 (53457,'Australasian & Pacific Art History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53453,1015,0),(53454,1015,0),(53455,1015,0),(53456,1015,0),(53457,1015,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('N6501','N6739',53453),('N6750','N7255',53454),
 ('N7260','N7372.72',53455),('N7380','N7399',53456),
 ('N7399.7','N7413',53457);

-- Civil and structural engineering subclass divisions. Reuse the established
-- environmental and railway engineering concepts through polyhierarchy.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (295,5564,1),(424,5564,1);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TC','TC',339),('TD','TD',295),('TE','TE',320),
 ('TF','TF',424),('TG','TG',196),('TH','TH',197);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (339,'TC'),(295,'TD'),(320,'TE'),(424,'TF'),(196,'TG'),(197,'TH');
DELETE FROM lcc_selector WHERE selector='TC' AND concept_id IN(1,5564);

-- Business reference block, previously retained inside a synthetic residual.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53460,'Business Directories'),(53461,'Business History'),
 (53462,'Business Handbooks');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53460,12265,0),(53461,12265,0),(53462,12265,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HF5030','HF5335.22',53460),('HF5341','HF5349',53461),
 ('HF5356','HF5363',53462);

-- Psychiatry divisions.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53463,'History of Psychiatry'),(53464,'Geriatric Psychiatry'),
 (53465,'Social & Community Psychiatry'),(53466,'Psychiatric Diagnosis'),
 (53467,'Psychoses'),(53468,'Neuroses'),
 (53469,'Developmental Disabilities');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53463,2837,0),(53464,2837,0),(53465,2837,0),(53466,2837,0),
 (53467,2837,0),(53468,2837,0),(53469,2837,0);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (5651,2837,1),(5448,2837,1),(2541,2837,1);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RC438','RC438.6',53463),('RC451.4.A5','RC451.4.A5',53464),
 ('RC455','RC455.4.Z',53465),('RC466.8','RC467.97',5651),
 ('RC469','RC473',53466),('RC475','RC489.2',5448),
 ('RC490','RC499',2541),('RC512','RC528',53467),
 ('RC530','RC552',53468),('RC569.7','RC574',53469);

-- Additional verified women-by-country codes from Table H9 remain exact,
-- because the generated country blocks are not safe contiguous region ranges.
-- Existing country children continue to receive their exact general-work code.

-- Christianity outside its history and theology ranges.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53470,'Philosophy of Christianity'),(53471,'Christianity & Society');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53470,4582,0),(53471,4582,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BR100','BR109.9',53470),('BR110','BR117.5',53471);

-- Psychology: fill major gaps using established children.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BF39','BF80',5653),('BF712','BF724.9',5391),
 ('BF173','BF175.5',5431),('BF231','BF299',5456),
 ('BF370','BF387',10805),('BF431','BF433.2',7018),
 ('BF501','BF505',5413),('BF608','BF635',5461),
 ('BF697','BF698.9',5425);

-- Visual arts reference, biography, media and criticism.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('N40','N44',5668),('N7430','N7433.85',5669),
 ('N7475','N7489.2',5670);

-- Television divisions.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('PN1992.7','PN1992.74',906),('PN1992.75','PN1992.76',897),
 ('PN1992.77','PN1992.79',898);

-- Additional high-use personnel Cutter topics.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53472,'Employee Performance Appraisal'),
 (53473,'International Personnel Management');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53472,12274,0),(53473,12274,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES
 (53472,'HF5549.5.R3'),(53473,'HF5549.5.E45');

-- Linguistics education and theory relationships.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53474,'Linguistics Education & Research'),
 (53475,'Language, Culture & Anthropology');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53474,3778,0),(53475,3778,0);
UPDATE lcc_range SET concept_id=53474
WHERE concept_id=3778 AND start_code='P51' AND end_code='P59.4';
INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES('P35','P35.5',53475);

-- Political science has already been partitioned below its honest JA/JA1-92
-- whole-schedule values; no arbitrary reassignment of those values is made.

-- Communication and media general/theory sections.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53476,'Mass Media Theory'),(53477,'Media Analysis & Models');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53476,4408,0),(53477,4408,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('P90','P91.26',53476),('P93','P93.7',53477);

-- Industries: high-use residual industry codes.
INSERT INTO concept(concept_id,preferred_label)
VALUES(53478,'Defense Industries');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(53478,5157,0);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5160,'HD9731.4'),(12204,'HD9685'),(53478,'HD9743'),(5234,'HD8039');
INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES('HD9743','HD9744',53478);

-- Ethics beyond its historical schedule.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BJ983','BJ1179',11079),('BJ1296','BJ1517.9',4504),
 ('BJ1518','BJ1697.2',4499);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (11079,'BJ1012'),(4504,'BJ1401'),(11079,'BJ1458.3'),(4499,'BJ1476');

-- Nursing major schedule divisions.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53479,'Nursing by Region'),(53480,'Nursing History & Biography'),
 (53481,'Nursing Profession');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53479,2747,0),(53480,2747,0),(53481,2747,0);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RT4','RT17',53479),('RT23','RT37',53480),
 ('RT40','RT55',5925),('RT71','RT81.5',5929),
 ('RT82','RT86',53481),('RT86.7','RT88.9',5928);

-- Electrical engineering gaps and communication technologies.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53482,'Electrical Engineering History & Reference'),
 (53483,'Electrical Engineering Education'),
 (53484,'Radio Engineering'),(53485,'Television Engineering'),
 (53486,'Electrical Appliances & Alarms');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53482,250,0),(53483,250,0),(53484,250,0),(53485,250,0),(53486,250,0);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(422,250,1);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TK1','TK159',53482),('TK165','TK255',53483),
 ('TK6540','TK6571.5',53484),('TK6573','TK6595',422),
 ('TK6630','TK6687',53485),('TK7018','TK7301',53486);

-- Photography history, technique, aesthetics and digital photography.
INSERT INTO concept(concept_id,preferred_label)
VALUES(53487,'Digital Photography');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(53487,915,0);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TR1','TR99',927),('TR100','TR179',5851),
 ('TR180','TR199',925),('TR267','TR267.5',53487);

-- Accounting functional divisions.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53488,'Accounting History'),(53489,'Auditing'),
 (53490,'Specialized Accounting'),(53491,'Accounting Technology');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53488,5105,0),(53489,5105,0),(53490,5105,0),(53491,5105,0);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HF5605','HF5611',53488),('HF5631','HF5655',12807),
 ('HF5657','HF5657.9',12810),('HF5667','HF5668.252',53489),
 ('HF5671','HF5677',53490),('HF5679','HF5681',53491);

-- Ecology: high-use named environments and special aspects use exact Cutter
-- selectors, while the existing parent range remains the general fallback.
INSERT INTO concept(concept_id,preferred_label) VALUES
 (53492,'Biodiversity'),(53493,'Landscape Ecology'),
 (53494,'Ecological Monitoring'),(53495,'Urban Ecology'),
 (53496,'Freshwater Ecology'),(53497,'Marine Ecology');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (53492,4790,0),(53493,4790,0),(53494,4790,0),
 (53495,4790,0),(53496,4790,0),(53497,4790,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES
 (53492,'QH541.15.B56'),(53493,'QH541.15.L35'),
 (53494,'QH541.15.M64'),(53495,'QH541.5.C6'),
 (53496,'QH541.5.F7'),(53497,'QH541.5.S3');
