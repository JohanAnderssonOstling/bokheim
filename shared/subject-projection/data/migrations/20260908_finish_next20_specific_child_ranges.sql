-- Finish the reviewed next-20 list. Only specific official ranges move down;
-- Biomedical Engineering remains unchanged because its children are interleaved
-- Cutter topics rather than safe contiguous numeric subranges.
BEGIN;

-- German government period.
DELETE FROM lcc_range WHERE concept_id=11604
 AND start_code='JN3201' AND end_code='JN4980';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JN3201','JN3356.99',11604),('JN3357','JN3444',52866),
 ('JN3444.1','JN4980',11604);

-- British government institutions.
DELETE FROM lcc_range WHERE concept_id=11598
 AND start_code='JN101' AND end_code='JN1371';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JN101','JN330.99',11598),('JN331','JN409',52664),
 ('JN331','JN389',52663),('JN409.1','JN424.99',11598),
 ('JN425','JN453',52665),('JN453.1','JN499.99',11598),
 ('JN500','JN695',52666),('JN617','JN653',52667),
 ('JN671','JN695',52668),('JN695.1','JN749.99',11598),
 ('JN750','JN841',52669),('JN841.1','JN1000.99',11598),
 ('JN1001','JN1033',52671),('JN1033.1','JN1371',11598);

-- Children and child development.
DELETE FROM lcc_range WHERE concept_id=12281
 AND start_code='HQ767.8' AND end_code='HQ792.2';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HQ767.8','HQ767.99',12281),('HQ768','HQ778.7',52435),
 ('HQ771','HQ772',52436),('HQ773.6','HQ773.7',52437),
 ('HQ778.5','HQ778.7',52438),('HQ778.71','HQ780.99',12281),
 ('HQ781','HQ784',52439),('HQ784.1','HQ788.99',12281),
 ('HQ789','HQ792.2',52440);

-- Government subclasses. JF remains the general/comparative administrative
-- schedule; country/region and local-government subclasses move down.
DELETE FROM lcc_range WHERE concept_id=5691
 AND start_code='JF' AND end_code='JS';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JF','JF',5691),('JJ','JJ',11923),('JK','JK',5327),
 ('JL','JL',11917),('JN','JN',5368),('JQ','JQ',5691),
 ('JS','JS',5359);

-- Marketing.
DELETE FROM lcc_range WHERE concept_id=5189
 AND start_code='HF5410' AND end_code='HF5418.9';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HF5410','HF5415.1259',12801),
 ('HF5415.126','HF5415.1264',12803),
 ('HF5415.126','HF5415.1262',12802),
 ('HF5415.1265','HF5415.1269',12806),
 ('HF5415.127','HF5415.1999',5189),
 ('HF5415.2','HF5415.34',12805),
 ('HF5415.35','HF5418.9',5189);

-- Chemistry.
DELETE FROM lcc_range WHERE concept_id=4803
 AND start_code='QD1' AND end_code='QD999';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('QD1','QD65',53336),('QD65.1','QD70.99',4803),
 ('QD71','QD142',4804),('QD142.1','QD145.99',4803),
 ('QD146','QD197',4806),('QD197.1','QD240.99',4803),
 ('QD241','QD441',4807),('QD441.1','QD449.99',4803),
 ('QD450','QD882',4808),('QD882.1','QD900.99',4803),
 ('QD901','QD999',4805);
DELETE FROM lcc_selector WHERE
 (concept_id=4804 AND selector='QD71-142') OR
 (concept_id=4806 AND selector='QD146-197') OR
 (concept_id=4807 AND selector='QD241-441') OR
 (concept_id=4808 AND selector='QD450-882') OR
 (concept_id=4805 AND selector='QD901-999');

-- Investments and securities.
DELETE FROM lcc_range WHERE concept_id=5183
 AND start_code='HG4501' AND end_code='HG6052.Z';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HG4501','HG4528.9',12758),('HG4529','HG4529.9',12759),
 ('HG4530','HG4537.9',12760),('HG4538','HG4550.9',12761),
 ('HG4551','HG4598.9',12762),('HG4599','HG4649.9',12763),
 ('HG4650','HG4660.9',12764),('HG4661','HG4700.9',12765),
 ('HG4701','HG4750.9',12766),('HG4751','HG4899.9',12767),
 ('HG4900','HG5999.9',12768),('HG5999.91','HG6000.99',5183),
 ('HG6001','HG6052.Z',12769),('HG6052','HG6052.Z',51543);
DELETE FROM lcc_selector WHERE concept_id IN(12758,12768)
 AND selector IN('HG4501-4528.9','HG4900-5999.9');

-- Human and civil rights.
DELETE FROM lcc_range WHERE concept_id=12531
 AND start_code='JC571' AND end_code='JC607';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JC571','JC584.99',12531),('JC585','JC607',52587),
 ('JC585','JC599',52586),('JC596','JC596.2',52588);

-- The 20260907 accounting split is replayed after the wrapper-collapse file,
-- so remove its now-redundant empty Specialized Accounting concept here.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 52584,selector FROM lcc_selector WHERE concept_id=53490;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 52584,selector FROM bisac_selector WHERE concept_id=53490;
UPDATE lcc_range SET concept_id=52584 WHERE concept_id=53490;
DELETE FROM lcc_selector WHERE concept_id=53490;
DELETE FROM bisac_selector WHERE concept_id=53490;
DELETE FROM source_label WHERE concept_id=53490;
DELETE FROM concept_parent WHERE concept_id=53490 OR parent_concept_id=53490;
DELETE FROM concept WHERE concept_id=53490;

COMMIT;
