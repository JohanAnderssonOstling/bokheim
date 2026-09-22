-- Remove cross-schedule umbrellas where specific established children already
-- own the corresponding LCC subclasses. Retain uncovered/general schedules on
-- their current parent; do not manufacture exact selectors.
BEGIN;

-- Agriculture retains the general S schedule. Its named SB/SD/SF children
-- already own plant production, forestry, and animal husbandry respectively.
DELETE FROM lcc_range WHERE concept_id=2
 AND start_code='S' AND end_code='SF';

-- Arts & Media is navigational. Established children own the actual arts
-- schedules (N/NA, M/MT, TR, BH, and the relevant PN blocks).
DELETE FROM lcc_range WHERE concept_id=510
 AND start_code='N' AND end_code='NX';

-- Religion keeps BL (general religion) itself. The remaining subclasses have
-- specific established homes: BM Judaism, BP Islam/Baha'i, BQ Buddhism,
-- BR Christianity, BS Bible, and BX denominations.
DELETE FROM lcc_range WHERE concept_id=4549
 AND start_code='BL' AND end_code='BX';

-- Geography, Places & Travel: distribute the covered schedules to existing
-- children and leave the otherwise uncovered GD and GF classes at the parent.
DELETE FROM lcc_range WHERE concept_id=2368
 AND start_code='G' AND end_code='GF';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GD','GD',2368),('GF','GF',2368);

-- Education: the official reports, museums, and directory divisions are
-- already represented by children. Preserve only the general gaps on Education.
DELETE FROM lcc_range WHERE concept_id=2191
 AND start_code='L1' AND end_code='L991';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('L1','L6.99',2191),('L98','L110.99',2191),
 ('L792','L796.99',2191),('L899','L899.99',2191);

COMMIT;
