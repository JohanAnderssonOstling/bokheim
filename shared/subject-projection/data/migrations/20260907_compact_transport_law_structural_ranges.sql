-- Consolidate official carriers/carriage schedules by jurisdiction under the
-- Trade, Transportation & Maritime Law leaf.
BEGIN;

DELETE FROM source_selector
WHERE concept_id=5618 AND system_id='lcc' AND selector IN (
  'KD1802','KD1804','KE1100','KE1102','KE1135',
  'KIB918','KIB920','KIB922'
);

DELETE FROM lcc_range
WHERE concept_id=5618 AND (class_letters,start_value,end_value) IN (
  VALUES
    ('KD','1811','1834'),('KD','1836','1842'),('KD','1845','1847'),
    ('KDC','530','532'),('KDK','435','437'),
    ('KE','1105','1114'),('KE','1121','1126'),
    ('KEO','339','340')
);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('KD','1800','1847',5618),
  ('KDC','526','532',5618),
  ('KDK','430','437',5618),
  ('KE','1099','1135',5618),
  ('KEO','336','340',5618),
  ('KIB','916','922',5618);

COMMIT;
