-- Materialize the distinct languages represented by the existing official LCC
-- ranges. Reuse the existing Hindi/Urdu/Hindustani sibling rather than duplicate it.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:indo-aryan-language:assamese','Assamese'),
 ('bokheim:indo-aryan-language:bengali','Bengali'),
 ('bokheim:indo-aryan-language:bihari','Bihari'),
 ('bokheim:indo-aryan-language:gujarati','Gujarati'),
 ('bokheim:indo-aryan-language:lahnda','Lahnda'),
 ('bokheim:indo-aryan-language:marathi','Marathi'),
 ('bokheim:indo-aryan-language:marwari','Marwari'),
 ('bokheim:indo-aryan-language:oriya','Odia (Oriya)'),
 ('bokheim:indo-aryan-language:pahari','Pahari'),
 ('bokheim:indo-aryan-language:panjabi','Panjabi'),
 ('bokheim:indo-aryan-language:rajasthani','Rajasthani'),
 ('bokheim:indo-aryan-language:sindhi','Sindhi'),
 ('bokheim:indo-aryan-language:sinhalese','Sinhalese'),
 ('bokheim:indo-aryan-language:siraiki','Siraiki'),
 ('bokheim:indo-aryan-language:romani','Romani');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:indo-aryan-language:assamese','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,100),
 ('bokheim:indo-aryan-language:bengali','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,200),
 ('bokheim:indo-aryan-language:bihari','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,300),
 ('bokheim:indo-aryan-language:gujarati','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,400),
 ('bokheim:indo-aryan-language:lahnda','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,500),
 ('bokheim:indo-aryan-language:marathi','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,600),
 ('bokheim:indo-aryan-language:marwari','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,700),
 ('bokheim:indo-aryan-language:oriya','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,800),
 ('bokheim:indo-aryan-language:pahari','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,900),
 ('bokheim:indo-aryan-language:panjabi','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,1000),
 ('bokheim:indo-aryan-language:rajasthani','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,1100),
 ('bokheim:indo-aryan-language:sindhi','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,1200),
 ('bokheim:indo-aryan-language:sinhalese','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,1300),
 ('bokheim:indo-aryan-language:siraiki','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,1400),
 ('bokheim:indo-aryan-language:romani','unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458',0,1500);
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:assamese',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK1550..PK1569@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:bengali',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK1651..PK1799@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:bihari',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK1801..PK1831.95@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:gujarati',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK1841..PK1870.95@5';
UPDATE source_selector SET concept_id='unified:c02e1693-cbe6-5f2f-a1b4-a12dd90a9a47',ordinal=20 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK1931..PK2212@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:lahnda',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2261..PK2270.9@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:marathi',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2351..PK2418@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:marwari',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2461..PK2479@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:oriya',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2561..PK2579.5@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:pahari',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2591..PK2610@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:panjabi',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2631..PK2659@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:rajasthani',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2701..PK2708.9@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:sindhi',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2781..PK2790@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:sinhalese',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2801..PK2891@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:siraiki',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2892..PK2892.95@5';
UPDATE source_selector SET concept_id='bokheim:indo-aryan-language:romani',ordinal=0 WHERE concept_id='unified:e63830a4-dc15-5c35-9cb3-a6eb4b452458' AND selector='PK2896..PK2899@5';
COMMIT;
