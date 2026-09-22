PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE eu_ranges(
 concept_id TEXT PRIMARY KEY,
 selector TEXT NOT NULL UNIQUE
) WITHOUT ROWID;
INSERT INTO eu_ranges VALUES
('bokheim:politics:europe:great-britain','JN101..JN1371@2'),
('bokheim:politics:europe:ireland','JN1395..JN1599@2'),
('bokheim:politics:europe:austria','JN1601..JN2049@2'),
('bokheim:politics:europe:czech-republic','JN2200..JN2229@2'),
('bokheim:politics:europe:france','JN2301..JN3007@2'),
('bokheim:politics:europe:germany','JN3201..JN4980@2'),
('bokheim:politics:europe:greece','JN5001..JN5191@2'),
('bokheim:politics:europe:italy','JN5201..JN5690@2'),
('bokheim:politics:europe:netherlands','JN5701..JN5999@2'),
('bokheim:politics:europe:belgium','JN6101..JN6372.22@2'),
('bokheim:politics:europe:northern-europe','JN7011..JN7066@2'),
('bokheim:politics:europe:denmark','JN7101..JN7367@2'),
('bokheim:politics:europe:norway','JN7401..JN7695@2'),
('bokheim:politics:europe:sweden','JN7721..JN7997@2'),
('bokheim:politics:europe:spain','JN8101..JN8399@2'),
('bokheim:politics:europe:portugal','JN8423..JN8661@2'),
('bokheim:politics:europe:switzerland','JN8701..JN9599@2'),
('bokheim:politics:europe:balkans','JN9600..JN9609@2'),
('bokheim:politics:europe:soviet-union','JN6500..JN6599@2');

DELETE FROM source_selector
WHERE concept_id IN (SELECT concept_id FROM eu_ranges);
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'lcc',selector,0 FROM eu_ranges;

COMMIT;
