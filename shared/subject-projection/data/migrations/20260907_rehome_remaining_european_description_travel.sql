BEGIN;

INSERT INTO concept(preferred_label)
SELECT 'Croatian Travel'
WHERE NOT EXISTS (SELECT 1 FROM concept WHERE preferred_label='Croatian Travel');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,2455,0 FROM concept WHERE preferred_label='Croatian Travel';

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 2493,v FROM (SELECT 'DK510.29' v UNION ALL SELECT 'DK754');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13972,v FROM (SELECT 'DK508.2' v UNION ALL SELECT 'DK508.926');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (13996,'DK502.4');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 2464,v FROM (
 SELECT 'DG975.S3' v UNION ALL SELECT 'DG821' UNION ALL SELECT 'DG601' UNION ALL
 SELECT 'DG843' UNION ALL SELECT 'DG863' UNION ALL SELECT 'DG653' UNION ALL
 SELECT 'DG733' UNION ALL SELECT 'DG692' UNION ALL SELECT 'DG613');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES (2461,'DA11');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 2460,v FROM (SELECT 'DD280.8' v UNION ALL SELECT 'DD801.W634');
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DR1817' FROM concept WHERE preferred_label='Montenegrin Travel';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,v FROM concept CROSS JOIN
 (SELECT 'DR1622' v UNION ALL SELECT 'DR1635')
WHERE preferred_label='Croatian Travel';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13958,'DK504.926';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13957,'DK503.926';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13959,'DK505.926';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 51886,'DK507.926';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 51920,'DK509.926';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13893,'DR279.6';
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,'DR49.23' FROM concept WHERE preferred_label='Balkan Travel';

COMMIT;
