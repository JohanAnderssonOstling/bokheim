BEGIN;
DELETE FROM lcc_range WHERE concept_id IN (5221,13006,12898,13086,11598,6791,12399);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5221,'HJ2240-5908'),(13006,'HG4001-4285'),(12898,'HV8130-8280.7'),
 (13086,'GF1-100.9'),(11598,'JN101-1371'),(6791,'HM1176-1281'),
 (12399,'HG2397-3550'),(12399,'HG4301-4480.9');

CREATE TEMP TABLE _split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('U.S. Taxation',5221,'HJ2360','HJ2442'),('Latin American Taxation',5221,'HJ2460.5','HJ2599'),('European Taxation',5221,'HJ2599.5','HJ2889'),('Tax Administration & Procedure',5221,'HJ3231','HJ3696'),('Direct Taxation',5221,'HJ3863','HJ3925'),('Property Taxation',5221,'HJ4113','HJ4601'),('Land & Real-Property Taxation',5221,'HJ4165','HJ4460'),('Income Taxation',5221,'HJ4629','HJ4830'),('Government Fees & User Charges',5221,'HJ5309','HJ5510'),
 ('U.S. Corporate Finance',13006,'HG4050','HG4070.2'),('Corporate Finance by Country',13006,'HG4090','HG4280.7'),
 ('History of Policing',12898,'HV8135','HV8141'),('Policing outside the U.S.',12898,'HV8157','HV8280.9'),
 ('Human-Ecology Theory & Methods',13086,'GF21','GF23'),('Human-Ecology Research & Education',13086,'GF26','GF28'),('Human Ecology before 1970',13086,'GF31','GF37'),('Modern Human Ecology',13086,'GF41','GF48'),('Humans & Specific Environments',13086,'GF52.2','GF67'),('Humans & Aquatic Environments',13086,'GF63','GF67'),
 ('British Government by Period',11598,'JN131','JN231'),('British Crown',11598,'JN331','JN389'),('British Executive Branch',11598,'JN331','JN409'),('British Civil Service',11598,'JN425','JN453'),('British Parliament',11598,'JN500','JN695'),('House of Lords',11598,'JN617','JN653'),('House of Commons',11598,'JN671','JN695'),('British Judiciary',11598,'JN750','JN841'),('British Political Participation',11598,'JN900','JN1088'),('British Election Law',11598,'JN1001','JN1033'),('Scottish Government',11598,'JN1187','JN1371'),
 ('Attitudes',6791,'HM1181','HM1241'),('Attitude Change',6791,'HM1186','HM1201'),('Mass Media & Social Influence',6791,'HM1206','HM1211'),('Authority',6791,'HM1251','HM1253'),('Leadership & Social Influence',6791,'HM1261','HM1263'),
 ('Banking in the Americas',12399,'HG2397','HG3542.7'),('North American Banking',12399,'HG2398','HG2626'),('U.S. Banking',12399,'HG2401','HG2626'),('European Banking',12399,'HG2971','HG3250.8'),('Trust Services & Companies',12399,'HG4301','HG4480.9'),('U.S. Trust Companies',12399,'HG4341','HG4356'),('Trust Companies by Country',12399,'HG4357','HG4480.9');

INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
