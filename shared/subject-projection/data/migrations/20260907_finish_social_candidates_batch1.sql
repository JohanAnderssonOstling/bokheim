BEGIN;

UPDATE concept SET preferred_label='Family Studies' WHERE concept_id=11801;
UPDATE concept SET preferred_label='Strategic Management' WHERE concept_id=11589;

-- Preserve all observed umbrella spellings exactly before removing permissive
-- ranges from these now-navigational parents.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||substr(end_code,length(rtrim(end_code,'0123456789.'))+1)
FROM lcc_range WHERE concept_id IN (5516,5391,11801,5097,11593,11589);
DELETE FROM lcc_range WHERE concept_id IN (5516,5391,11801,5097,11593,11589);

CREATE TEMP TABLE _split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('History of Social Conditions',5516,'HN8','HN19'),('Social-Conditions Theory & Methods',5516,'HN28','HN28.2'),('Religion & Social Problems',5516,'HN30','HN39'),('Community & Social Centers',5516,'HN41','HN46'),('U.S. Social Conditions',5516,'HN51','HN90'),('Social Reform',5516,'HN61','HN65'),('Social Conditions outside the U.S.',5516,'HN101','HN942.7'),('European Social Conditions',5516,'HN371','HN650.8'),('Communist Social Conditions',5516,'HN958','HN962'),('Developing-Country Social Conditions',5516,'HN978','HN981'),('Tropical Social Conditions',5516,'HN990','HN991'),
 ('Developmental Assessment',5391,'BF719.6','BF719.65'),('Child Psychology',5391,'BF721','BF723'),('Developmental-Psychology Theory',5391,'BF722','BF722.35'),('Adult Development',5391,'BF724.5','BF724.85'),('Psychology of Aging',5391,'BF724.8','BF724.85'),
 ('History of Family Life',11801,'HQ503','HQ519'),('Family Life in Antiquity',11801,'HQ505','HQ512'),('Modern Family Life',11801,'HQ515','HQ518'),('Family Studies by Region',11801,'HQ531','HQ727'),('North American Family Studies',11801,'HQ533','HQ557'),('U.S. Family Studies',11801,'HQ535','HQ557'),('Family Treatises & Home Life',11801,'HQ728','HQ743'),
 ('Economics as a Science',5097,'HB71','HB74'),('Economic Information & Communication',5097,'HB74.2','HB74.4'),('Economics Education & Research',5097,'HB74.5','HB74.9'),
 ('Delegation & Decentralization',11593,'HD50','HD50.5'),('Organizational Effectiveness',11593,'HD58.9','HD58.95'),('Workplace Teams',11593,'HD66','HD66.5'),
 ('Management Theory & Methods',11589,'HD30.19','HD30.29'),('Management Communication',11589,'HD30.3','HD30.36'),('Management Education & Research',11589,'HD30.4','HD30.42'),('History of Management',11589,'HD30.5','HD30.65'),('Executive Management',11589,'HD38.1532','HD38.25');

INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
