BEGIN;
-- JV7590-JV8349 is the actual European schedule.  The former leaf extended
-- through JV8489.9, immediately before the Asian schedule, and therefore
-- swallowed unrelated residual numbers.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (12445,'JV7590-8349');
DELETE FROM lcc_range WHERE concept_id=12445;

CREATE TEMP TABLE _migration_split(label TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _migration_split VALUES
 ('Migration in England','JV7600','JV7695'),
 ('Migration in Scotland','JV7700','JV7709'),
 ('Migration in Ireland','JV7710','JV7719'),
 ('Migration in Wales','JV7720','JV7729'),
 ('Migration in Austria','JV7800','JV7895'),
 ('Migration in France','JV7900','JV7995'),
 ('Migration in Germany','JV8000','JV8095'),
 ('Migration in Greece','JV8110','JV8119'),
 ('Migration in Italy','JV8130','JV8139'),
 ('Migration in the Low Countries','JV8149','JV8175'),
 ('Migration in Russia & Former Soviet Republics','JV8180','JV8189'),
 ('Migration in the Baltic States','JV8192.5','JV8193'),
 ('Migration in Scandinavia','JV8198','JV8229'),
 ('Migration in Spain','JV8250','JV8259'),
 ('Migration in Portugal','JV8260','JV8269'),
 ('Migration in Switzerland','JV8280','JV8289'),
 ('Migration in the Balkans','JV8295','JV8349');

INSERT INTO concept(preferred_label)
SELECT label FROM _migration_split s
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12445,0 FROM _migration_split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _migration_split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _migration_split;
COMMIT;
