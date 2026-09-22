PRAGMA foreign_keys=ON;
BEGIN;
CREATE TEMP TABLE _ddc_map(selector TEXT PRIMARY KEY, concept_id INTEGER NOT NULL);
INSERT INTO _ddc_map VALUES
 ('320',5369),
 ('321',5370),
 ('322',5696),
 ('323',5330),
 ('324',11782),
 ('325',5332),
 ('326',12240),
 ('327',5693),
 ('328',52395);
INSERT OR REPLACE INTO ddc_selector(selector,concept_id)
SELECT selector,concept_id FROM _ddc_map;
DROP TABLE _ddc_map;
COMMIT;
