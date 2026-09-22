BEGIN;

WITH wanted(label) AS (VALUES
 ('Zhejiang History'),('Jiangsu History'),('Yunnan History'),('Henan History'),
 ('Sichuan History'),('Gansu History'),('Shandong History'),('Anhui History'),
 ('Guangdong History'),('Shanxi History'),('Shaanxi History'),('Fujian History'),
 ('Hebei History'),('Hubei History'),('Xinjiang History'))
INSERT INTO concept(preferred_label)
SELECT label FROM wanted
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label=label);

-- Province leaves sit beside the residual broad locality leaf. More-specific
-- Nanjing and Guangzhou exact selectors retain priority within their province.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,3493,0 FROM concept WHERE preferred_label IN (
 'Zhejiang History','Jiangsu History','Yunnan History','Henan History',
 'Sichuan History','Gansu History','Shandong History','Anhui History',
 'Guangdong History','Shanxi History','Shaanxi History','Fujian History',
 'Hebei History','Hubei History','Xinjiang History');

INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.88.A','797.88.Z',concept_id FROM concept WHERE preferred_label='Zhejiang History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.56.A','797.56.Z',concept_id FROM concept WHERE preferred_label='Jiangsu History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.86.A','797.86.Z',concept_id FROM concept WHERE preferred_label='Yunnan History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.44.A','797.44.Z',concept_id FROM concept WHERE preferred_label='Henan History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.77.A','797.77.Z',concept_id FROM concept WHERE preferred_label='Sichuan History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.28.A','797.28.Z',concept_id FROM concept WHERE preferred_label='Gansu History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.72.A','797.72.Z',concept_id FROM concept WHERE preferred_label='Shandong History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.22.A','797.22.Z',concept_id FROM concept WHERE preferred_label='Anhui History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.32.A','797.32.Z',concept_id FROM concept WHERE preferred_label='Guangdong History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.75.A','797.75.Z',concept_id FROM concept WHERE preferred_label='Shanxi History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.68.A','797.68.Z',concept_id FROM concept WHERE preferred_label='Shaanxi History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.26.A','797.26.Z',concept_id FROM concept WHERE preferred_label='Fujian History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.39.A','797.39.Z',concept_id FROM concept WHERE preferred_label='Hebei History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.48.A','797.48.Z',concept_id FROM concept WHERE preferred_label='Hubei History';
INSERT OR IGNORE INTO lcc_range SELECT 'DS','797.84.A','797.84.Z',concept_id FROM concept WHERE preferred_label='Xinjiang History';

COMMIT;
