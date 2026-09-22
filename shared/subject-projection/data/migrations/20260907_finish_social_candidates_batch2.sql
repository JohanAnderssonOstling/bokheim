BEGIN;

-- Fully covered country trees: retain only literal regional notations.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (11988,'HC240-407'),(11989,'HC411-495'),(11990,'HC800-1085');
DELETE FROM lcc_range WHERE concept_id IN (11988,11989,11990);

-- Convert remaining parents to exact umbrella notations.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (11994,'HC101-110'),(12282,'HQ793-799.49'),(12403,'HQ72.8-78.9'),
 (11595,'HD62.2-62.8'),(11595,'HD62.4-62.45'),
 (5142,'HD39-40.7'),(5142,'HG1-199'),(5142,'HG1-9999');
DELETE FROM lcc_range WHERE concept_id IN (11994,12282,12403,11595,5142);

CREATE TEMP TABLE _split(label TEXT,parent_id,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('U.S. Economy, Colonial Period',11994,'HC104','HC104'),('U.S. Economy, 1776–1900',11994,'HC105','HC105.7'),('U.S. Economy, 1901–1945',11994,'HC106','HC106.4'),('U.S. Economy, 1945–1961',11994,'HC106.5','HC106.5'),('U.S. Economy, 1961–1971',11994,'HC106.6','HC106.6'),('U.S. Economy, 1971–1981',11994,'HC106.7','HC106.7'),('U.S. Economy, 1981–1993',11994,'HC106.8','HC106.8'),('U.S. Economy, 1993–2000',11994,'HC106.82','HC106.82'),('U.S. Economy, 2001–2009',11994,'HC106.83','HC106.83'),('U.S. Economy since 2009',11994,'HC106.84','HC106.84'),
 ('Youth in the U.S.',12282,'HQ796','HQ798'),('Young Men & Women',12282,'HQ799.5','HQ799.9'),('Adulthood',12282,'HQ799.95','HQ799.97'),
 ('Sexual Minorities',12403,'HQ73','HQ78.2'),('Bisexuality',12403,'HQ74','HQ74.9'),('Gay & Lesbian Studies',12403,'HQ75','HQ76.965'),('Same-Sex Relationships',12403,'HQ76.34','HQ76.35'),('Homophobia & Heterosexism',12403,'HQ76.4','HQ76.45'),('LGBTQ+ Rights Movements',12403,'HQ76.5','HQ76.8'),('LGBTQ+ Culture & Press',12403,'HQ76.9','HQ76.965'),('Transgender Studies',12403,'HQ76.97','HQ77.9'),('Transphobia & Discrimination',12403,'HQ77.96','HQ77.965'),('Intersex Studies',12403,'HQ77.97','HQ78.2'),
 ('Special-Enterprise Management',11595,'HD62.2','HD62.8'),('International Enterprise Management',11595,'HD62.4','HD62.45'),
 ('Finance Reference & Directories',5142,'HG1','HG96'),('Finance Theory & Methods',5142,'HG101','HG106'),('Finance Education & Research',5142,'HG152','HG152.5'),('Financial Statistics',5142,'HG176','HG176.5'),('Financial-Services Discrimination',5142,'HG176.6','HG176.63'),('Fundraising',5142,'HG177','HG177.5'),('Hawala Finance',5142,'HG177.7','HG177.8'),('Microfinance',5142,'HG178.3','HG178.33'),('Finance by Region',5142,'HG181','HG190');

INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;

-- Project management and capital investment already have appropriate leaves.
INSERT INTO concept(preferred_label) SELECT 'Project Management'
WHERE NOT EXISTS(SELECT 1 FROM concept WHERE preferred_label='Project Management');
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,11595,0 FROM concept WHERE preferred_label='Project Management';
UPDATE lcc_selector SET concept_id=(SELECT concept_id FROM concept WHERE preferred_label='Project Management')
WHERE concept_id=11595 AND selector='HD69.P75';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES ('HD39','HD40.7',5183);
UPDATE lcc_selector SET concept_id=5127 WHERE concept_id=5142 AND selector='HD60.7';

COMMIT;
