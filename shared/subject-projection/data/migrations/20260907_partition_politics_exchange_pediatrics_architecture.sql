-- Continue the reviewed candidate list with broad LCC subranges only.
-- General and unclassified intervals remain at their current parent.
BEGIN;

-- The political-science root keeps only class J general material. Major
-- subclass families move to the corresponding established broad subjects.
DELETE FROM lcc_range WHERE concept_id=5326
 AND start_code='J' AND end_code='JZ';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('J','J',5326),('JA','JC',5690),('JF','JS',5691),
 ('JV','JV',5693),('JX','JZ',5693);

-- Foreign exchange and international finance.
DELETE FROM lcc_range WHERE concept_id=5147
 AND start_code='HG3810' AND end_code='HG4000';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HG3810','HG3810.99',5147),('HG3811','HG3815',52466),
 ('HG3815.1','HG3820.99',5147),('HG3821','HG3823',52467),
 ('HG3823.1','HG3851.29',5147),('HG3851.3','HG3851.32',52468),
 ('HG3851.33','HG3860.99',5147),('HG3861','HG3873',52469),
 ('HG3873.1','HG3878.99',5147),('HG3879','HG3881.99',52470),
 ('HG3882','HG3890',52471),('HG3890.1','HG3893.99',52470),
 ('HG3894','HG3898.2',52472),('HG3898.21','HG3900.99',52470),
 ('HG3901','HG4000',52473);
DELETE FROM lcc_selector WHERE concept_id=52470 AND selector='HG3879-3898';

-- Pediatric diseases: regional studies and psychotherapy are substantial
-- contiguous divisions. The mixed diagnostic/psychiatric core remains here,
-- with its precise existing child selectors continuing to win.
DELETE FROM lcc_range WHERE concept_id=2811
 AND start_code='RJ499' AND end_code='RJ507';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RJ499','RJ500.99',2811),('RJ501','RJ502',9052),
 ('RJ502.01','RJ503.99',2811),('RJ504','RJ505',10044),
 ('RJ505.01','RJ507',2811);

-- Architecture theory and practice. Only official specific divisions move;
-- all surrounding theory/practice material remains at the parent.
DELETE FROM lcc_range WHERE concept_id=5698;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('NA100','NA130',559),('NA130.1','NA150',5698),
 ('NA2500','NA2500',51698),('NA2500.1','NA2599.49',5698),
 ('NA2599.5','NA2599.9',557),('NA2600','NA2698.99',5698),
 ('NA2699','NA2700',571),('NA2700.1','NA4050',5698);

COMMIT;
