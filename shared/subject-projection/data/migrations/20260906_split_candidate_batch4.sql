-- Five compact candidates split into selector-bearing leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Law Enforcement & Police
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:law-enforcement:general','General Law Enforcement'),
 ('bokheim:law-enforcement:administration','Police Administration & Organization'),
 ('bokheim:law-enforcement:duties','Police Duties & Protective Methods'),
 ('bokheim:law-enforcement:private','Private Detectives & Detective Bureaus'),
 ('bokheim:law-enforcement:regional','Police by Region or Country');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:law-enforcement:general','unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4',0,100),
 ('bokheim:law-enforcement:administration','unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4',0,200),
 ('bokheim:law-enforcement:duties','unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4',0,300),
 ('bokheim:law-enforcement:private','unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4',0,400),
 ('bokheim:law-enforcement:regional','unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4',0,500);
UPDATE source_selector SET concept_id='bokheim:law-enforcement:general',ordinal=0 WHERE concept_id='unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4' AND selector='POL014000';
UPDATE source_selector SET concept_id='bokheim:law-enforcement:administration',ordinal=0 WHERE concept_id='unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4' AND selector='HV7935..HV8025@4';
UPDATE source_selector SET concept_id='bokheim:law-enforcement:duties',ordinal=0 WHERE concept_id='unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4' AND selector='HV8031..HV8080@4';
UPDATE source_selector SET concept_id='bokheim:law-enforcement:private',ordinal=0 WHERE concept_id='unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4' AND selector='HV8081..HV8099@4';
UPDATE source_selector SET concept_id='bokheim:law-enforcement:regional',ordinal=0 WHERE concept_id='unified:b1f5439f-4b60-55d5-94c5-353a31a4d6a4' AND selector='HV8130..HV8280.7@4';

-- Flowers & Ornamental Plants
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:flowers:general','General Flower Gardening'),('bokheim:flowers:annuals','Annuals'),
 ('bokheim:flowers:bulbs','Bulbs'),('bokheim:flowers:orchids','Orchids'),
 ('bokheim:flowers:perennials','Perennials'),('bokheim:flowers:roses','Roses'),
 ('bokheim:flowers:wildflowers','Wildflowers'),('bokheim:flowers:ornamental','Ornamental Plants'),
 ('bokheim:flowers:shrubs','Shrubs'),('bokheim:flowers:topiary','Topiary');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:flowers:general','bokheim:horticulture:flowers-ornamentals',0,100),('bokheim:flowers:annuals','bokheim:horticulture:flowers-ornamentals',0,200),
 ('bokheim:flowers:bulbs','bokheim:horticulture:flowers-ornamentals',0,300),('bokheim:flowers:orchids','bokheim:horticulture:flowers-ornamentals',0,400),
 ('bokheim:flowers:perennials','bokheim:horticulture:flowers-ornamentals',0,500),('bokheim:flowers:roses','bokheim:horticulture:flowers-ornamentals',0,600),
 ('bokheim:flowers:wildflowers','bokheim:horticulture:flowers-ornamentals',0,700),('bokheim:flowers:ornamental','bokheim:horticulture:flowers-ornamentals',0,800),
 ('bokheim:flowers:shrubs','bokheim:horticulture:flowers-ornamentals',0,900),('bokheim:flowers:topiary','bokheim:horticulture:flowers-ornamentals',0,1000);
UPDATE source_selector SET concept_id='bokheim:flowers:general',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND system_id='bisac' AND selector='GAR004000';
UPDATE source_selector SET concept_id='bokheim:flowers:general',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND system_id='lcc';
UPDATE source_selector SET concept_id='bokheim:flowers:annuals',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR004010';
UPDATE source_selector SET concept_id='bokheim:flowers:bulbs',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR004030';
UPDATE source_selector SET concept_id='bokheim:flowers:orchids',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR004040';
UPDATE source_selector SET concept_id='bokheim:flowers:perennials',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR004050';
UPDATE source_selector SET concept_id='bokheim:flowers:roses',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR004060';
UPDATE source_selector SET concept_id='bokheim:flowers:wildflowers',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR004080';
UPDATE source_selector SET concept_id='bokheim:flowers:ornamental',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR017000';
UPDATE source_selector SET concept_id='bokheim:flowers:shrubs',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR021000';
UPDATE source_selector SET concept_id='bokheim:flowers:topiary',ordinal=0 WHERE concept_id='bokheim:horticulture:flowers-ornamentals' AND selector='GAR023000';

-- Catholic Worship, Devotion & Religious Life
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:catholic-life:creeds','Creeds & Catechisms'),('bokheim:catholic-life:worship','Worship, Meditation & Devotion'),
 ('bokheim:catholic-life:sacraments','Sacraments & Sacramentals'),('bokheim:catholic-life:saints-places','Saints, Shrines, Pilgrimages & Relics'),
 ('bokheim:catholic-life:christian-life','Christian Life, Service & Religious Vocation');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:catholic-life:creeds','bokheim:catholic:worship-devotion-religious-life',0,100),('bokheim:catholic-life:worship','bokheim:catholic:worship-devotion-religious-life',0,200),
 ('bokheim:catholic-life:sacraments','bokheim:catholic:worship-devotion-religious-life',0,300),('bokheim:catholic-life:saints-places','bokheim:catholic:worship-devotion-religious-life',0,400),
 ('bokheim:catholic-life:christian-life','bokheim:catholic:worship-devotion-religious-life',0,500);
UPDATE source_selector SET concept_id='bokheim:catholic-life:creeds',ordinal=0 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX1958..BX1968@4';
UPDATE source_selector SET concept_id='bokheim:catholic-life:worship',ordinal=0 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX1969';
UPDATE source_selector SET concept_id='bokheim:catholic-life:worship',ordinal=1 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2177..BX2199.2@4';
UPDATE source_selector SET concept_id='bokheim:catholic-life:sacraments',ordinal=0 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2200..BX2292@4';
UPDATE source_selector SET concept_id='bokheim:catholic-life:sacraments',ordinal=1 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2295..BX2310@4';
UPDATE source_selector SET concept_id='bokheim:catholic-life:saints-places',ordinal=0 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2312';
UPDATE source_selector SET concept_id='bokheim:catholic-life:saints-places',ordinal=1 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2315';
UPDATE source_selector SET concept_id='bokheim:catholic-life:saints-places',ordinal=CASE selector
 WHEN 'BX2320..BX2321@4' THEN 2 WHEN 'BX2323..BX2323.52@4' THEN 3
 WHEN 'BX2324..BX2324.2@4' THEN 4 WHEN 'BX2325..BX2334.2@4' THEN 5
 WHEN 'BX2340' THEN 6 WHEN 'BX2345' THEN 7 END
 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector IN ('BX2320..BX2321@4','BX2323..BX2323.52@4','BX2324..BX2324.2@4','BX2325..BX2334.2@4','BX2340','BX2345');
UPDATE source_selector SET concept_id='bokheim:catholic-life:christian-life',ordinal=0 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2347..BX2348@4';
UPDATE source_selector SET concept_id='bokheim:catholic-life:christian-life',ordinal=1 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2349..BX2377@4';
UPDATE source_selector SET concept_id='bokheim:catholic-life:christian-life',ordinal=2 WHERE concept_id='bokheim:catholic:worship-devotion-religious-life' AND selector='BX2380..BX2387.2@4';

-- History of Economic Thought
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:economic-thought:general','General History of Economic Thought'),('bokheim:economic-thought:period','Economic Thought by Period'),
 ('bokheim:economic-thought:schools','Schools of Economic Thought'),('bokheim:economic-thought:regions','Economic Thought by Region or Country');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:economic-thought:general','unified:9d86501e-f3dd-566c-851c-217d989f32d7',0,100),('bokheim:economic-thought:period','unified:9d86501e-f3dd-566c-851c-217d989f32d7',0,200),
 ('bokheim:economic-thought:schools','unified:9d86501e-f3dd-566c-851c-217d989f32d7',0,300),('bokheim:economic-thought:regions','unified:9d86501e-f3dd-566c-851c-217d989f32d7',0,400);
DELETE FROM source_selector WHERE concept_id='unified:9d86501e-f3dd-566c-851c-217d989f32d7';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:economic-thought:general','lcc','HB75..HB76.9@4',0),('bokheim:economic-thought:general','lcc','HB75-130',1),
 ('bokheim:economic-thought:period','lcc','HB77..HB89.9@4',0),('bokheim:economic-thought:schools','lcc','HB90..HB100.9@4',0),
 ('bokheim:economic-thought:schools','lcc','HB90-99.722',1),('bokheim:economic-thought:regions','lcc','HB101..HB130@4',0);

-- Jurisprudence, Philosophy & Natural Law
INSERT INTO concept(concept_id,preferred_label) VALUES
 ('bokheim:jurisprudence:general','General, Methodology & History'),('bokheim:jurisprudence:concept','Concept of Law'),
 ('bokheim:jurisprudence:acts-sources','Legal Acts, Events & Sources'),('bokheim:jurisprudence:interpretation','Legal Interpretation & Classification'),
 ('bokheim:jurisprudence:schools','Schools of Legal Theory & Natural Law'),('bokheim:jurisprudence:relations','Law in Relation to Other Topics');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
 ('bokheim:jurisprudence:general','bokheim:subject:law:systems:philosophy',0,100),('bokheim:jurisprudence:concept','bokheim:subject:law:systems:philosophy',0,200),
 ('bokheim:jurisprudence:acts-sources','bokheim:subject:law:systems:philosophy',0,300),('bokheim:jurisprudence:interpretation','bokheim:subject:law:systems:philosophy',0,400),
 ('bokheim:jurisprudence:schools','bokheim:subject:law:systems:philosophy',0,500),('bokheim:jurisprudence:relations','bokheim:subject:law:systems:philosophy',0,600);
DELETE FROM source_selector WHERE concept_id='bokheim:subject:law:systems:philosophy';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal) VALUES
 ('bokheim:jurisprudence:general','bisac','LAW052000',0),('bokheim:jurisprudence:general','lcc','K201..K236.9@2',0),
 ('bokheim:jurisprudence:concept','lcc','K237..K269.9@2',0),('bokheim:jurisprudence:acts-sources','lcc','K270..K287.9@2',0),
 ('bokheim:jurisprudence:interpretation','lcc','K288..K320.9@2',0),('bokheim:jurisprudence:schools','bisac','LAW069000',0),
 ('bokheim:jurisprudence:schools','lcc','K321..K485.9@2',0),('bokheim:jurisprudence:relations','lcc','K486..K487@2',0);

COMMIT;
