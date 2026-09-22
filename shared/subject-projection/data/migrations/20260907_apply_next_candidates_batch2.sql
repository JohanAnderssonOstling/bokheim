BEGIN;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT concept_id,start_code||'-'||substr(end_code,length(rtrim(end_code,'0123456789.'))+1)
FROM lcc_range WHERE concept_id IN (12837,12240,12294,12768);
DELETE FROM lcc_range WHERE concept_id IN (12837,12240,12294,12768);

CREATE TEMP TABLE _split(label TEXT,parent_id,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('History of Money',12837,'HG231','HG256'),('Medieval Monetary History',12837,'HG241','HG243'),('Money in the 16th–18th Centuries',12837,'HG248','HG249'),('Money in the 20th Century',12837,'HG255','HG256'),
 ('U.S. Slavery',12240,'E441','E453'),('Religion & Slavery',12240,'HT910','HT921'),('Slavery & Law',12240,'HT941','HT950'),('Slave Trade',12240,'HT973.2','HT999'),('History of Slavery',12240,'HT975','HT985.2'),('Abolition & Anti-Slavery',12240,'HT1024.2','HT1037.2'),('Slavery by Region',12240,'HT1048','HT1445'),('Slavery in South America',12240,'HT1121','HT1152'),('Slavery in Europe',12240,'HT1155','HT1240'),('Slavery in Asia',12240,'HT1240.5','HT1315'),('Captivity Slavery',12240,'HT1345','HT1365'),
 ('Middle Age',12294,'HQ1059.4','HQ1059.5'),('Older People & Social Gerontology',12294,'HQ1060','HQ1064'),('Retirement',12294,'HQ1062','HQ1063'),
 ('Investment in the Americas',12768,'HG4900','HG5131'),('North American Investment',12768,'HG4901','HG5131'),('U.S. Investment',12768,'HG4904.2','HG5131'),('Government Securities',12768,'HG4931','HG4955'),('Investment outside the Americas',12768,'HG5151','HG5992.7'),('European Investment',12768,'HG5421','HG5700.8');
INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
