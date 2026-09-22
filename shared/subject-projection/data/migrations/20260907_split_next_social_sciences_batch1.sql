BEGIN;

UPDATE concept SET preferred_label='Marketing' WHERE concept_id=12801;
UPDATE concept SET preferred_label='Accounting & Bookkeeping' WHERE concept_id=12807;
DELETE FROM lcc_range WHERE concept_id IN (52384,12801,12807,12531,5200,5377);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (52384,'HV6250.92-6773.42'),(12801,'HF5410-5417.5'),
 (12807,'HF5601-5688'),(12807,'HF5601-5689'),
 (12531,'JC571-607'),(5200,'HF5546-5548.6'),(5377,'HV6430-6433.88');

CREATE TEMP TABLE _split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('State & Transnational Crime',52384,'HV6251.6','HV6252.5'),
 ('Political Crime',52384,'HV6254','HV6322.3'),
 ('Crimes Against Justice',52384,'HV6323','HV6335'),
 ('Revenue & Tax Crimes',52384,'HV6337','HV6351'),
 ('Environmental & Wildlife Crime',52384,'HV6401','HV6412'),
 ('Public-Safety Offenses',52384,'HV6419','HV6433'),
 ('Organized Crime & Gangs',52384,'HV6435','HV6492'),
 ('Crimes Against Persons',52384,'HV6493','HV6626.7'),
 ('Homicide',52384,'HV6499','HV6535'),
 ('Mass Shootings',52384,'HV6536','HV6536.6'),
 ('Suicide',52384,'HV6543','HV6548'),
 ('Sex Crimes',52384,'HV6556','HV6593'),
 ('Kidnapping & Abduction',52384,'HV6571','HV6604'),
 ('Stalking',52384,'HV6594','HV6594.3'),
 ('Public-Morals Offenses',52384,'HV6705','HV6738'),
 ('Gambling',52384,'HV6708','HV6722'),

 ('Marketing Management',12801,'HF5415.13','HF5415.16'),
 ('Marketing Research',12801,'HF5415.2','HF5415.34'),
 ('Consumer Behavior',12801,'HF5415.32','HF5415.34'),
 ('Customer Service & Relations',12801,'HF5415.5','HF5415.53'),
 ('Goods Distribution',12801,'HF5415.6','HF5415.9'),
 ('Pricing',12801,'HF5416.5','HF5417'),

 ('History of Accounting',12807,'HF5605','HF5611'),
 ('Accounting Theory & Methods',12807,'HF5625','HF5625.15'),
 ('Accounting Information',12807,'HF5625.5','HF5625.7'),
 ('Accounting Standards',12807,'HF5626','HF5626.2'),
 ('Accounting Firms',12807,'HF5628','HF5628.5'),
 ('Auditing',12807,'HF5667','HF5668.252'),
 ('Internal Auditing',12807,'HF5668','HF5668.25'),
 ('Specialized Accounting',12807,'HF5671','HF5677'),
 ('Accounts & Bookkeeping',12807,'HF5680','HF5681'),

 ('Liberty & Freedom',12531,'JC585','JC599'),
 ('Individual Rights',12531,'JC585','JC607'),
 ('Privacy Rights',12531,'JC596','JC596.2'),

 ('Office Automation',5200,'HF5548','HF5548.6'),
 ('Office Data Processing',5200,'HF5548.125','HF5548.6'),
 ('Office Electronic Commerce',5200,'HF5548.32','HF5548.33'),

 ('U.S. Terrorism',5377,'HV6432','HV6432.7'),
 ('Terrorism by Region',5377,'HV6432','HV6433'),
 ('Bioterrorism',5377,'HV6433.3','HV6433.5'),
 ('Chemical Terrorism',5377,'HV6433.7','HV6433.78'),
 ('Narco-Terrorism',5377,'HV6433.79','HV6433.8'),
 ('Nuclear Terrorism',5377,'HV6433.85','HV6433.88');

INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;

-- Existing crime leaves receive their official ranges.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HV6626','HV6626.23',13274),('HV6626.5','HV6626.54',13275),
 ('HV6635','HV6700',12729),('HV6763','HV6771',12730),
 ('HV6772','HV6773.3',12731);

COMMIT;
