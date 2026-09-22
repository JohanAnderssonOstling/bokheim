-- Extract only established, specific LCC subranges. General residual material
-- remains on the current parent and no generated exact-code lists are added.
BEGIN;

-- Microbiology.
DELETE FROM lcc_range WHERE concept_id=4975;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('QR1','QR74.79',4975),('QR74.8','QR99.5',4976),
 ('QR99.6','QR99.8',4977),('QR99.81','QR99.99',4975),
 ('QR100','QR130',4979),('QR130.1','QR179.99',4975),
 ('QR180','QR189.5',4978),('QR189.6','QR354.99',4975),
 ('QR355','QR502',4980);

-- Child psychology: the theory block is specific; the surrounding general
-- child-psychology material stays on the parent.
DELETE FROM lcc_range WHERE concept_id=10989
 AND start_code='BF721' AND end_code='BF723';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BF721','BF721.99',10989),('BF722','BF722.35',11033),
 ('BF722.36','BF723',10989);

-- Old and New Testament schedules have natural homes in the two existing
-- children. Preserve only the inter-schedule gap on their combined parent.
DELETE FROM lcc_range WHERE concept_id=5729;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BS701','BS1830',12372),('BS1831','BS1900.99',5729),
 ('BS1901','BS2970',12373);

-- Corporate finance by jurisdiction.
DELETE FROM lcc_range WHERE concept_id=13006
 AND start_code='HG4001' AND end_code='HG4285';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HG4001','HG4049.99',13006),('HG4050','HG4070.2',52652),
 ('HG4070.3','HG4089.99',13006),('HG4090','HG4280.7',52653),
 ('HG4280.8','HG4285',13006);

-- Character and virtue ethics: isolate the established vice schedule while
-- retaining general virtue and character ethics on the parent.
DELETE FROM lcc_range WHERE concept_id=4499
 AND start_code='BJ1518' AND end_code='BJ1697.2';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BJ1518','BJ1534.99',4499),('BJ1535.A','BJ1535.Z',8238),
 ('BJ1536','BJ1697.2',4499);

-- Public and social customs. Named festival/custom blocks move to the existing
-- children; unspecialized gaps remain on Public & Social Customs.
DELETE FROM lcc_range WHERE concept_id=12313
 AND start_code='GT3400' AND end_code='GT5090';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GT3400','GT3599.99',12313),('GT3600','GT3660',52517),
 ('GT3660.1','GT3979.99',12313),('GT3980','GT4096',52522),
 ('GT4096.1','GT4179.99',12313),('GT4180','GT4296',52523),
 ('GT4296.1','GT4379.99',12313),('GT4380','GT4496',52524),
 ('GT4496.1','GT4579.99',12313),('GT4580','GT4696',52525),
 ('GT4696.1','GT4904.99',12313),('GT4905','GT4908',52526),
 ('GT4908.1','GT4927.99',12313),('GT4928','GT4930',52527),
 ('GT4930.1','GT4934.99',12313),('GT4935','GT4935.5',52528),
 ('GT4935.6','GT4984.99',12313),('GT4985','GT4992',52529),
 ('GT4992.1','GT5049.99',12313),('GT5050','GT5051',52531),
 ('GT5051.1','GT5090',12313);

COMMIT;
