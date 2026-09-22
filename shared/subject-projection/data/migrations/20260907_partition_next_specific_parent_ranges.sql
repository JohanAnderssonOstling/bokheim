-- Partition high-use parent schedules only where the LCC schedule defines a
-- specific subject. Introductory, reference, and otherwise general intervals
-- remain on their existing parent. Use structural ranges rather than expanding
-- the schedule into generated exact selectors.
BEGIN;

-- Computer networks: TK5105.5-.9 has several stable subject divisions.
INSERT INTO concept(concept_id,preferred_label)
SELECT 54001,'Computer Network Architecture' WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label='Computer Network Architecture');
INSERT INTO concept(concept_id,preferred_label)
SELECT 54002,'Computer Network Hardware' WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label='Computer Network Hardware');
INSERT INTO concept(concept_id,preferred_label)
SELECT 54003,'Network Protocols & Standards' WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label='Network Protocols & Standards');
INSERT INTO concept(preferred_label)
SELECT 'Local Area Networks' WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label='Local Area Networks');
INSERT INTO concept(concept_id,preferred_label)
SELECT 54004,'Wide Area Networks' WHERE NOT EXISTS
 (SELECT 1 FROM concept WHERE preferred_label='Wide Area Networks');

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,8486,0 FROM concept WHERE preferred_label IN
 ('Computer Network Architecture','Computer Network Hardware',
  'Network Protocols & Standards','Local Area Networks','Wide Area Networks');

DELETE FROM lcc_range WHERE concept_id=8486
 AND start_code='TK5105.5' AND end_code='TK5105.9';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TK5105.5','TK5105.5199',8486),
 ('TK5105.526','TK5105.5299',8486),
 ('TK5105.547','TK5105.5499',8486),
 ('TK5105.581','TK5105.6999',8486),
 ('TK5105.744','TK5105.8699',8486),
 ('TK5105.8888','TK5105.9',8486);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'TK5105.52','TK5105.525',concept_id FROM concept WHERE preferred_label='Computer Network Architecture';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'TK5105.53','TK5105.546',concept_id FROM concept WHERE preferred_label='Computer Network Hardware';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'TK5105.55','TK5105.58',concept_id FROM concept WHERE preferred_label='Network Protocols & Standards';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'TK5105.7','TK5105.743',concept_id FROM concept WHERE preferred_label='Local Area Networks';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT 'TK5105.87','TK5105.8887',concept_id FROM concept WHERE preferred_label='Wide Area Networks';

-- Pediatrics: keep reference, practice, and unspecialized gaps at Pediatrics;
-- assign only the official named clinical divisions to established children.
DELETE FROM lcc_range WHERE concept_id=2809 AND start_code='RJ1' AND end_code='RJ570';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RJ1','RJ49.99',2809),('RJ54','RJ58.99',2809),
 ('RJ61','RJ100.99',2809),('RJ104.3','RJ124.99',2809),
 ('RJ138','RJ205.99',2809),('RJ236','RJ244.99',2809),
 ('RJ248','RJ249.99',2809),('RJ250.31','RJ250.99',2809),
 ('RJ326','RJ369.99',2809),('RJ521','RJ570',2809),
 ('RJ101','RJ104.2',2810),('RJ125','RJ137',2820),
 ('RJ206','RJ235',2819),('RJ245','RJ247',2818),
 ('RJ251','RJ325',2817),('RJ370','RJ520',2811);

-- Oncology: retain its general pathology/reference areas at the parent.
DELETE FROM lcc_range WHERE concept_id=2712
 AND start_code='RC254' AND end_code='RC282';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RC254','RC260.99',2712),('RC272','RC275.99',2712),
 ('RC280','RC282',2712),('RC261','RC271',6749),
 ('RC276','RC279',9524);

-- Biology: QH301-358 is the general/research portion and stays on Biology.
-- The official named biological disciplines below it already have children.
DELETE FROM lcc_range WHERE concept_id=4789
 AND start_code='QH301' AND end_code='QH705';
DELETE FROM lcc_range WHERE concept_id=13696;
DELETE FROM lcc_selector WHERE concept_id=13696 AND selector='QH301-QH355';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('QH301','QH358.99',4789),('QH359','QH425',4793),
 ('QH426','QH470',4794),('QH471','QH489',4796),
 ('QH491','QH492',13697),('QH493','QH500.99',4789),
 ('QH501','QH531',4795),('QH532','QH539.99',4789),
 ('QH540','QH549.5',4790),('QH549.6','QH572.99',4789),
 ('QH573','QH671',4791),('QH671.1','QH704.99',4789),
 ('QH705','QH705.5',4792);

-- Physics: preserve the general/reference and genuine gaps on Physics, while
-- assigning official major divisions to its established subject children.
DELETE FROM lcc_range WHERE concept_id=5030
 AND start_code='QC1' AND end_code='QC999';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('QC1','QC119.99',5030),('QC198','QC220.99',5030),
 ('QC247','QC349.99',5030),('QC468','QC473.99',5030),
 ('QC497','QC500.99',5030),('QC767','QC769.99',5030),
 ('QC799','QC999',5030),('QC120','QC168.86',4871),
 ('QC170','QC197',5032),('QC221','QC246',5031),
 ('QC350','QC467',5040),('QC474','QC496.9',5041),
 ('QC501','QC766',5034),('QC770','QC798',5039);
DELETE FROM lcc_selector WHERE concept_id IN (4871,5032,5031,5040,5041,5034,5039)
 AND selector IN ('QC120-168.86','QC170-197','QC221-246','QC350-467',
                  'QC474-496.9','QC501-766','QC770-798');

COMMIT;
