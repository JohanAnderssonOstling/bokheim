-- These ranges are wholly contained by a non-direct descendant through a
-- navigation node. Preserve their printed notation as exact parent selectors.
BEGIN;

CREATE TEMP TABLE _covered(concept_id INTEGER,start_code TEXT,end_code TEXT,PRIMARY KEY(concept_id,start_code,end_code));
INSERT INTO _covered VALUES
 (3407,'E303','E309'),(3602,'DQ131','DQ151'),
 (3705,'PL5051','PL5497.95'),(3705,'PL5501','PL6135'),(3705,'PL6145','PL6167.95'),
 (3705,'PL6191','PL6341'),(3705,'PL6401','PL6551.5'),(3705,'PL6601','PL6621'),(3705,'PL7001','PL7101'),
 (4460,'PG3223','PG3229.2'),(4460,'PG3230','PG3238'),(4460,'PG3240','PG3255'),(4460,'PG3260','PG3299'),
 (4628,'GV1018','GV1019'),(4628,'GV1019.2','GV1038.2'),(4628,'GV1040','GV1059'),
 (4628,'GV1059.5','GV1060.4'),(4628,'GV1060.5','GV1098'),(4628,'GV1151','GV1190'),
 (4628,'GV1195','GV1198.995'),(4628,'GV565','GV565.2'),(4628,'GV567.5','GV568.3'),
 (4628,'GV568.69','GV568.7'),(4628,'GV571','GV689.2'),(4628,'GV691','GV693'),
 (4628,'GV701','GV705'),(4628,'GV706','GV706.7'),(4628,'GV708','GV709.7'),
 (4628,'GV711.7','GV711.82'),(4628,'GV712','GV725'),(4628,'GV733.52','GV734.5'),
 (4628,'GV742','GV742.42'),(4628,'GV743','GV749'),(4628,'GV750','GV770.28'),
 (4628,'GV770.3','GV840'),(4628,'GV840.7','GV857'),(4628,'GV858.2','GV859.7'),
 (4628,'GV859.73','GV859.76'),(4628,'GV859.8','GV859.813'),
 (4916,'QA10.92','QA20'),(4916,'QA8.9','QA10.3'),(5250,'HQ74','HQ74.2'),
 (5729,'BS1937','BS2020.82'),(5729,'BS2025','BS2213.72'),
 (5729,'BS705','BS815'),(5729,'BS824.82','BS1013.52');

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r JOIN _covered x USING(concept_id,start_code,end_code);

DELETE FROM lcc_range
WHERE (concept_id,start_code,end_code) IN (SELECT concept_id,start_code,end_code FROM _covered);

DROP TABLE _covered;
COMMIT;
