BEGIN;

WITH wanted(label) AS (VALUES
 ('Egyptian Historical Reference'),
 ('Egyptian Antiquities'),
 ('Egyptian Civilization & Ethnography'),
 ('Egyptian Military History'),
 ('Egypt, 1517–1798'),
 ('Nile History'),
 ('Cairo History'),
 ('Egyptian Local History Outside Cairo & the Nile'),
 ('North African Regional History'),
 ('North African Travel'),
 ('Vandal & Byzantine North Africa, 439–c. 640'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

UPDATE concept SET preferred_label='Egypt, 638–1250' WHERE concept_id=10415;
UPDATE concept SET preferred_label='Egypt, 1250–1517' WHERE concept_id=10678;
UPDATE concept SET preferred_label='Egypt, 1798–1879' WHERE concept_id=10296;
UPDATE concept SET preferred_label='Egypt, 1879–1952' WHERE concept_id=10521;
UPDATE concept SET preferred_label='Egypt, 1952–2011' WHERE concept_id=10277;
UPDATE concept SET preferred_label='Egypt, 2011–Present' WHERE concept_id=10767;

-- Remove every permissive range from the nonleaf geographic umbrella and the
-- obsolete Egypt-wide "Modern Era" node. The ranges are rebuilt below as a
-- mutually exclusive partition owned by meaningful leaves.
DELETE FROM lcc_range WHERE concept_id IN (3357,10316);

-- Egypt-wide children.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3635,0 FROM concept WHERE preferred_label IN (
 'Egyptian Historical Reference','Egyptian Antiquities',
 'Egyptian Civilization & Ethnography','Egyptian Military History',
 'Egypt, 1517–1798','Nile History','Cairo History',
 'Egyptian Local History Outside Cairo & the Nile');

-- Ancient Egypt is shared by the established Ancient History arm and Egypt.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(3481,3635,1);
DELETE FROM lcc_range WHERE concept_id=10711;
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES('DT','57','65',3481),('DT','83','93',3481);

-- Flatten the old Egypt-specific "Modern Era" navigation node into Egypt.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT cp.concept_id,3635,0 FROM concept_parent cp WHERE cp.parent_concept_id=10316;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(10767,3635,0);

-- Egypt political and diplomatic topics remain reachable thematically.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(10358,10277,1),(10612,10277,1),(10612,51078,2);

-- General/reference and travel portions of DT43–DT56.2.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','43','44.999',concept_id FROM concept WHERE preferred_label='Egyptian Historical Reference';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','46','49.97',concept_id FROM concept WHERE preferred_label='Egyptian Historical Reference';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','45','45.999',2486;
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','49.98','56.2',2486;

UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Egyptian Historical Reference')
WHERE concept_id=3635 AND selector IN ('DT43','DT44','DT46','DT48','DT49');
UPDATE lcc_selector SET concept_id=2486
WHERE concept_id=3635 AND selector IN
 ('DT45','DT49.98','DT50','DT51','DT53','DT54','DT55','DT56','DT56.2');

-- Antiquities, society, historical reference, military, and diplomacy.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','56.8','56.999',concept_id FROM concept WHERE preferred_label='Egyptian Antiquities';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','66','69.5',concept_id FROM concept WHERE preferred_label='Egyptian Antiquities';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','70','73.999',concept_id FROM concept WHERE preferred_label='Egyptian Civilization & Ethnography';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','74','80.999',concept_id FROM concept WHERE preferred_label='Egyptian Historical Reference';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Egyptian Military History')
WHERE concept_id=10349 AND selector='DT81';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','81','81.999',concept_id FROM concept WHERE preferred_label='Egyptian Military History';
UPDATE lcc_range SET concept_id=51078
WHERE concept_id=10336 AND class_letters='DT' AND start_value='82' AND end_value='82.5';

-- Broad Egyptian chronological leaves. Smaller dynasties, wars, and incidents
-- remain absorbed in these periods.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES(3635,'DT94');
DELETE FROM lcc_range WHERE concept_id=10415;
INSERT OR IGNORE INTO lcc_range VALUES('DT','95','95.88',10415);
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','97','98.5',concept_id FROM concept WHERE preferred_label='Egypt, 1517–1798';
DELETE FROM lcc_range WHERE concept_id=10767;
INSERT OR IGNORE INTO lcc_range VALUES('DT','107.88','107.999',10767);

-- Egyptian local history: Nile, Cairo, and the remaining localities.
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','115','117',concept_id FROM concept WHERE preferred_label='Nile History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','139','153.5',concept_id FROM concept WHERE preferred_label='Cairo History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','118','138.999',concept_id FROM concept
WHERE preferred_label='Egyptian Local History Outside Cairo & the Nile';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','153.6','154',concept_id FROM concept
WHERE preferred_label='Egyptian Local History Outside Cairo & the Nile';

-- Correct malformed and redundant Sudan ranges; the official national span is
-- exactly DT154.1–DT159.9.
DELETE FROM lcc_range WHERE concept_id=3368;
INSERT OR IGNORE INTO lcc_range VALUES('DT','154.1','159.9',3368);

-- Regional North Africa begins at DT160. Travel is separated from history.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3357,0 FROM concept WHERE preferred_label='North African Regional History';
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2427,0 FROM concept WHERE preferred_label='North African Travel';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','160','161.899',concept_id FROM concept WHERE preferred_label='North African Regional History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','162','162.999',concept_id FROM concept WHERE preferred_label='North African Regional History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','165.21','167.999',concept_id FROM concept WHERE preferred_label='North African Regional History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','177','177.999',concept_id FROM concept WHERE preferred_label='North African Regional History';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','161.9','161.999',concept_id FROM concept WHERE preferred_label='North African Travel';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','163','165.2',concept_id FROM concept WHERE preferred_label='North African Travel';

-- Flatten the regional "By Era" navigation node into North African History.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT cp.concept_id,3357,0 FROM concept_parent cp
WHERE cp.parent_concept_id=11445 AND cp.concept_id NOT IN (10711,10767,10316);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3357,0 FROM concept WHERE preferred_label='Vandal & Byzantine North Africa, 439–c. 640';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Vandal & Byzantine North Africa, 439–c. 640')
WHERE concept_id=11445 AND selector='DT172';
INSERT OR IGNORE INTO lcc_range(class_letters,start_value,end_value,concept_id)
SELECT 'DT','171','172.999',concept_id FROM concept
WHERE preferred_label='Vandal & Byzantine North Africa, 439–c. 640';
INSERT OR IGNORE INTO lcc_range VALUES('DT','170','170.999',10301);
INSERT OR IGNORE INTO lcc_range VALUES('DT','173','173.999',10667);
INSERT OR IGNORE INTO lcc_range VALUES('DT','174','175.999',10399);
INSERT OR IGNORE INTO lcc_range VALUES('DT','176','176.199',10548);
INSERT OR IGNORE INTO lcc_range VALUES('DT','176.2','176.999',10343);

-- Remove obsolete mislabelled/navigation concepts after preserving all of
-- their meaningful children and selectors.
DELETE FROM concept_parent WHERE parent_concept_id IN (10316,11445);
DELETE FROM concept_parent WHERE concept_id IN (10316,10336,10349,10711,11445);
DELETE FROM concept WHERE concept_id IN (10316,10336,10349,10711,11445);

-- Add these primary links after removing their former ordinal-0 navigation
-- parents; otherwise the per-concept ordinal uniqueness rule ignores them.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(10343,3357,0),(10767,3635,0);

COMMIT;
