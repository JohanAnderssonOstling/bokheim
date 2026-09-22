BEGIN;

-- KTA1-KTA9150 is the Nigerian law schedule. Give the complete schedule to a
-- jurisdiction leaf and remove redundant nested state ranges and anchors.
INSERT INTO concept(concept_id,preferred_label) VALUES(50830,'Nigerian Law');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES(50830,5276,0);

UPDATE lcc_range SET concept_id=50830
WHERE class_letters='KTA' AND start_value='1' AND end_value='9150';
DELETE FROM lcc_range
WHERE class_letters='KTA' AND (start_value,end_value) IN (
  VALUES('5200','8200'),('9100','9125')
);
DELETE FROM source_selector
WHERE concept_id=5645 AND system_id='lcc' AND selector GLOB 'KTA*';
UPDATE source_selector SET concept_id=50830
WHERE concept_id=5276 AND system_id='lcc' AND selector GLOB 'KTA*';

-- General Roman and European contract spans.
DELETE FROM source_selector
WHERE concept_id=5282 AND system_id='lcc' AND selector IN (
  'KJA2546','KJA2547','KJA2550','KJA2551',
  'KJC1724','KJC1726','KJC1728','KJC1730'
);
DELETE FROM lcc_range
WHERE concept_id=5282 AND (class_letters,start_value,end_value) IN (
  VALUES
    ('KJA','2548','2548.22'),
    ('KJC','1732','1760'),('KJC','1762','1764')
);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('KJA','2542','2590',5282),
  ('KJC','1720','1764.24',5282);

-- These selectors were filed under Contracts but describe labor contracts.
DELETE FROM source_selector
WHERE concept_id=5282 AND system_id='lcc' AND selector IN (
  'KJC2871','KJC2878','KJC2880','KJC2882','KJC2930',
  'KJE2888','KJE2922'
);
DELETE FROM lcc_range
WHERE (class_letters,start_value,end_value) IN (
  VALUES
    ('KJC','2870','2940'),
    ('KJC','2898','2911'),('KJC','2914','2929'),('KJC','2938','2940'),
    ('KJE','2872','2875')
);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES('KJC','2870','2940',5615);

-- German competition-control law belongs under IP & Competition Law.
DELETE FROM source_selector
WHERE concept_id=5282 AND system_id='lcc' AND selector IN (
  'KK6458','KK6462','KK6464','KK6530','KK6533'
);
DELETE FROM lcc_range
WHERE concept_id=5282 AND class_letters='KK' AND (start_value,end_value) IN (
  VALUES('6465','6494'),('6497','6504.2'),('6511','6529')
);
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id)
VALUES('KK','6456','6534',13314);

COMMIT;
