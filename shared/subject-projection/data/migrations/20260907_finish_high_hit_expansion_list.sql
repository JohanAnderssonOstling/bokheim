BEGIN;
CREATE TEMP TABLE _split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 -- Sports: each broad block aggregates hundreds of matched uses.
 ('Sports History, Culture & Reference',13641,'GV561','GV710.99'),
 ('Sports Coaching, Events & Administration',13641,'GV711','GV749.99'),
 ('Air, Water, Winter & Action Sports',13641,'GV750','GV860.99'),
 ('Ball Games',13641,'GV861','GV1017'),
 ('Racing, Cycling & Athletics',13641,'GV1018','GV1098'),
 ('Combat, Shooting & Wrestling Sports',13641,'GV1100','GV1198.995'),
 -- Neurology: four heavily used schedule divisions.
 ('Neurological Examination & Treatment',7901,'RC346','RC359.99'),
 ('Central Nervous System Diseases',7901,'RC360','RC408.99'),
 ('Peripheral Nerve Disorders',7901,'RC409','RC422.99'),
 ('Communication & Speech Disorders',7901,'RC423','RC429.2'),
 -- Textile arts.
 ('Sewing & Needlework',6600,'TT699','TT719.99'),
 ('Mending',6600,'TT720','TT739.99'),
 ('Decorative Needlework',6600,'TT740','TT834.99'),
 ('Quilting & Patchwork',6600,'TT835','TT847.99'),
 ('Hand Weaving',6600,'TT848','TT850.99'),
 ('Textile Decoration',6600,'TT851','TT854.5'),
 -- Biology and research.
 ('Biology Reference & General Works',13696,'QH301','QH314.99'),
 ('Biology Education & Laboratories',13696,'QH315','QH323.99'),
 ('Experimental Biology & Research Methods',13696,'QH324','QH326.99'),
 ('Space & Theoretical Biology',13696,'QH327','QH342.99'),
 ('Biogeochemistry & Biological Cycles',13696,'QH343','QH351.99'),
 ('Population Biology',13696,'QH352','QH355'),
 -- Personal health.
 ('Personal Health & Healthy Habits',6194,'RA773','RA780.99'),
 ('Exercise & Physical Wellness',6194,'RA781','RA785.99'),
 ('Sleep & Rest',6194,'RA786','RA788'),
 -- International economic relations.
 ('International Economic Relations — Foundations',52758,'HF1351','HF1370.99'),
 ('International Trade',52758,'HF1371','HF1409.99'),
 ('Foreign Trade Policy, Exports & Imports',52758,'HF1410','HF1449.99'),
 ('Regional International Commerce',52758,'HF1450','HF1647'),
 -- Electronics and engineering materials.
 ('Special Electronics Applications',8669,'TK7880','TK7884.99'),
 ('Computer Engineering & Hardware',8669,'TK7885','TK7895'),
 ('Nonmetallic Engineering Materials',7707,'TA418.95','TA455.2'),
 ('Metallic Engineering Materials',7707,'TA459','TA492.2'),
 -- Indoor games.
 ('Card Games',4665,'GV1232','GV1299'),
 ('Gambling & Games of Chance',4665,'GV1301','GV1311'),
 ('Board & Strategy Games',4665,'GV1312','GV1469'),
 ('Computer, Video & Fantasy Games',4665,'GV1469.15','GV1469.62'),
 -- Oncology.
 ('Cancer Foundations, History & Biography',6749,'RC261','RC267.99'),
 ('Cancer Prevention, Genetics & Carcinogenesis',6749,'RC268','RC269.99'),
 ('Cancer Diagnosis & Treatment',6749,'RC270','RC271'),
 -- Primary-school subjects.
 ('Primary Reading Education',6993,'LB1572','LB1574.99'),
 ('Primary Literature, Language & Composition',6993,'LB1575','LB1580.99'),
 ('Primary Humanities, Science & Health',6993,'LB1581','LB1588.99'),
 ('Primary Thinking Skills & Arts',6993,'LB1589','LB1593.99'),
 ('Primary Vocational Education & Study Methods',6993,'LB1594','LB1602'),
 -- Baking and confectionery.
 ('Baking Reference & Fundamentals',9966,'TX761','TX768.99'),
 ('Bread Making',9966,'TX769','TX770'),
 ('Cakes & Cake Decorating',9966,'TX771','TX771.2'),
 ('Cookies',9966,'TX772','TX772'),
 ('Desserts, Pies, Puddings & Pastry',9966,'TX773','TX780.19'),
 ('Confectionery',9966,'TX780.2','TX799');

-- Preserve the former umbrella notation only as an exact parent selector.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||end_code FROM lcc_range
WHERE concept_id IN (13641,7901,6600,13696,6194,52758,8669,7707,4665,6749,6993,9966);
DELETE FROM lcc_range WHERE concept_id IN
 (13641,7901,6600,13696,6194,52758,8669,7707,4665,6749,6993,9966);

INSERT INTO concept(preferred_label) SELECT DISTINCT label FROM _split s
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,COALESCE((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM (SELECT DISTINCT label,parent_id FROM _split) s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
