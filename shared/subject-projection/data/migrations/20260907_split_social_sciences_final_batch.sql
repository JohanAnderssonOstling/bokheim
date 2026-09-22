BEGIN;

-- Preserve broad catalogue notations exactly, then replace permissive parent
-- coverage with official numeric leaves.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5470,'HV85-525'),(12773,'HD166-1138.9'),(12432,'HD1748-2210.Z'),
 (12361,'HD6350-6940.7'),(5234,'HD4801-8943'),
 (5252,'HQ503-1064'),(5057,'GN1-890');
DELETE FROM lcc_range WHERE concept_id IN (5470,12773,12432,12361,5234,5252,5057);

CREATE TEMP TABLE _final_split(label TEXT,parent_id INTEGER,start_code TEXT,end_code TEXT);
INSERT INTO _final_split VALUES
 ('U.S. Social Welfare',5470,'HV85','HV99'),
 ('Social Welfare by Country',5470,'HV101','HV525'),

 ('Land Use in the Americas',12773,'HD166','HD279'),
 ('U.S. Land Use',12773,'HD170','HD279'),
 ('Public Lands',12773,'HD215.2','HD243.2'),
 ('Real Estate & Property',12773,'HD251','HD279'),
 ('Land Use in Other Countries',12773,'HD311','HD1130.7'),
 ('European Land Use',12773,'HD581','HD840.8'),
 ('British Land Use',12773,'HD591','HD620.5'),

 ('Agricultural Economics in the Americas',12432,'HD1748','HD1775'),
 ('North American Agricultural Economics',12432,'HD1750','HD1775'),
 ('U.S. Agricultural Economics',12432,'HD1751','HD1775'),
 ('Agricultural Economics by Country',12432,'HD1781','HD2200.7'),

 ('History of Trade Unions',12361,'HD6451','HD6481.2'),
 ('Guilds',12361,'HD6456','HD6473'),
 ('Trade-Union Topics',12361,'HD6484','HD6497'),
 ('U.S. Trade Unions',12361,'HD6500','HD6519'),
 ('Trade Unions by Country',12361,'HD6521','HD6940.5'),

 ('Women Workers',5234,'HD6050','HD6223'),
 ('Worker Groups',5234,'HD6050','HD6305'),
 ('Employment Agencies & Labor Exchanges',5234,'HD5860','HD6000.7'),
 ('Industrial Housing',5234,'HD7285','HD7391'),
 ('Model Workplaces',5234,'HD7406','HD7510'),
 ('Model Communities',5234,'HD7526','HD7630'),

 ('History of the Family',5252,'HQ503','HQ519'),
 ('Family by Region or Country',5252,'HQ531','HQ727'),
 ('North American Family Studies',5252,'HQ533','HQ557'),
 ('U.S. Family Studies',5252,'HQ535','HQ557'),
 ('Family Studies by Country',5252,'HQ559','HQ727'),
 ('Non-Monogamous Relationships',5252,'HQ980','HQ997'),

 ('Psychological Anthropology',5057,'GN502','GN519');

INSERT INTO concept(preferred_label)
SELECT DISTINCT s.label FROM _final_split s
WHERE NOT EXISTS (SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,0 FROM _final_split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _final_split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _final_split;

-- Route the broad labor divisions into existing curated children.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HD4909','HD5100.9',12356),('HD5106','HD5267',12357),
 ('HD5306','HD5474',12358),('HD5481','HD5630.7',12359),
 ('HD5701','HD5852',11823),('HD6977','HD7080',12364),
 ('HD7088','HD7252',11865),('HD7260','HD7780.8',12366),
 ('HD7795','HD8027',12367);

-- Route family divisions into existing substantive children.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HQ728','HQ743',11801),('HQ811','HQ960.7',12288);
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (12281,'HQ767.8-792.2');

-- Reuse existing anthropology leaves and the newly partitioned ethnic tree.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('GN49','GN298',5070),('GN62.8','GN265',5067),
 ('GN301','GN674',5061),('GN406','GN442',5068),
 ('GN451','GN477.7',5058),('GN478','GN491.7',5061),
 ('GN537','GN674',5060),('GN700','GN890',5071);

COMMIT;
