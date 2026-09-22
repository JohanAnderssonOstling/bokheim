PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 55. Biblical criticism and interpretation.
INSERT INTO concept VALUES
 ('bokheim:bible-criticism:general','Biblical Criticism & Interpretation—General'),
 ('bokheim:bible-criticism:old-testament','Old Testament Criticism & Interpretation'),
 ('bokheim:bible-criticism:new-testament','New Testament Criticism & Interpretation'),
 ('bokheim:bible-criticism:hermeneutics','Biblical Exegesis & Hermeneutics');
INSERT INTO concept_parent VALUES
 ('bokheim:bible-criticism:general','bokheim:bible:criticism-hermeneutics',0,100),
 ('bokheim:bible-criticism:old-testament','bokheim:bible:criticism-hermeneutics',0,200),
 ('bokheim:bible-criticism:new-testament','bokheim:bible:criticism-hermeneutics',0,300),
 ('bokheim:bible-criticism:hermeneutics','bokheim:bible:criticism-hermeneutics',0,400);
DELETE FROM source_selector WHERE concept_id='bokheim:bible:criticism-hermeneutics';
INSERT INTO source_selector VALUES
 ('bokheim:bible-criticism:general','bisac','REL006080',0),('bokheim:bible-criticism:general','lcc','BS500..BS534.8@3',0),
 ('bokheim:bible-criticism:old-testament','bisac','REL006090',0),('bokheim:bible-criticism:old-testament','lcc','BS1160..BS1191.5@3',0),
 ('bokheim:bible-criticism:new-testament','bisac','REL006100',0),('bokheim:bible-criticism:hermeneutics','bisac','REL006400',0);

-- 57. Analytical methods used for physical problems.
INSERT INTO concept VALUES
 ('bokheim:analytical-physics:general','Analytical Methods for Physical Problems—General'),
 ('bokheim:analytical-physics:control','Control Theory'),('bokheim:analytical-physics:optimization','Mathematical Optimization & Programming'),
 ('bokheim:analytical-physics:harmonic','Harmonic Analysis'),('bokheim:analytical-physics:fourier','Fourier Analysis'),
 ('bokheim:analytical-physics:special','Other Analytical Methods for Physical Problems');
INSERT INTO concept_parent VALUES
 ('bokheim:analytical-physics:general','bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36',0,100),
 ('bokheim:analytical-physics:control','bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36',0,200),
 ('bokheim:analytical-physics:optimization','bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36',0,300),
 ('bokheim:analytical-physics:harmonic','bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36',0,400),
 ('bokheim:analytical-physics:fourier','bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36',0,500),
 ('bokheim:analytical-physics:special','bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36',0,600);
DELETE FROM source_selector WHERE concept_id='bokheim:lcc-detail:18643f99-a898-5b3a-99a4-832ec998ee36';
INSERT INTO source_selector VALUES
 ('bokheim:analytical-physics:general','lcc','QA401..QA402.29@4',0),
 ('bokheim:analytical-physics:control','lcc','QA402.3..QA402.49@4',0),
 ('bokheim:analytical-physics:optimization','lcc','QA402.5..QA402.99@4',0),
 ('bokheim:analytical-physics:harmonic','lcc','QA403..QA403.49@4',0),
 ('bokheim:analytical-physics:fourier','lcc','QA403.5..QA404.5@4',0),
 ('bokheim:analytical-physics:special','lcc','QA404.6..QA425@4',0);

-- 58. Business cycles and economic fluctuations.
INSERT INTO concept VALUES
 ('bokheim:business-cycles:general','Business Cycles—General'),('bokheim:business-cycles:history','History of Economic Crises'),
 ('bokheim:business-cycles:relations','Business Cycles—Social Aspects & Related Topics'),
 ('bokheim:business-cycles:forecasting','Economic Forecasting'),('bokheim:business-cycles:stabilization','Economic Stabilization'),
 ('bokheim:business-cycles:regional','Business Cycles by Country or Region');
INSERT INTO concept_parent VALUES
 ('bokheim:business-cycles:general','unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84',0,100),
 ('bokheim:business-cycles:history','unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84',0,200),
 ('bokheim:business-cycles:relations','unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84',0,300),
 ('bokheim:business-cycles:forecasting','unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84',0,400),
 ('bokheim:business-cycles:stabilization','unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84',0,500),
 ('bokheim:business-cycles:regional','unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84',0,600);
DELETE FROM source_selector WHERE concept_id='unified:d0559e80-d3c9-5c2f-bd09-8958a66d2c84';
INSERT INTO source_selector VALUES
 ('bokheim:business-cycles:general','lcc','HB3711..HB3715.9@4',0),('bokheim:business-cycles:history','lcc','HB3716..HB3717.9@4',0),
 ('bokheim:business-cycles:relations','lcc','HB3718..HB3729.9@4',0),('bokheim:business-cycles:forecasting','lcc','HB3730..HB3731.9@4',0),
 ('bokheim:business-cycles:stabilization','lcc','HB3732..HB3740.9@4',0),('bokheim:business-cycles:regional','lcc','HB3741..HB3840@4',0);

-- 60. Plant health and protection.
INSERT INTO concept VALUES
 ('bokheim:plant-health:general','Plant Health, Pests & Diseases—General'),('bokheim:plant-health:weeds','Weeds & Parasitic Plants'),
 ('bokheim:plant-health:poisonous','Poisonous Plants'),('bokheim:plant-health:pathology','Plant Pathology'),
 ('bokheim:plant-health:entomology','Economic Entomology'),('bokheim:plant-health:control','Pest Control & Plant Protection');
INSERT INTO concept_parent VALUES
 ('bokheim:plant-health:general','bokheim:subject:plant-culture:health',0,100),('bokheim:plant-health:weeds','bokheim:subject:plant-culture:health',0,200),
 ('bokheim:plant-health:poisonous','bokheim:subject:plant-culture:health',0,300),('bokheim:plant-health:pathology','bokheim:subject:plant-culture:health',0,400),
 ('bokheim:plant-health:entomology','bokheim:subject:plant-culture:health',0,500),('bokheim:plant-health:control','bokheim:subject:plant-culture:health',0,600);
DELETE FROM source_selector WHERE concept_id='bokheim:subject:plant-culture:health';
INSERT INTO source_selector VALUES
 ('bokheim:plant-health:general','lcc','SB599..SB609.9@3',0),('bokheim:plant-health:weeds','lcc','SB610..SB616.9@3',0),
 ('bokheim:plant-health:poisonous','lcc','SB617..SB620.9@3',0),('bokheim:plant-health:pathology','lcc','SB621..SB817.9@3',0),
 ('bokheim:plant-health:entomology','lcc','SB818..SB949.9@3',0),
 ('bokheim:plant-health:control','bisac','TEC058000',0),('bokheim:plant-health:control','lcc','SB950..SB990.5@3',0);

COMMIT;
