BEGIN;

-- Preserve the former broad notations as exact parent matches.  Permissive
-- numeric ranges are moved to schedule-backed leaves below.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (11824,'HD5721-5852'),(12528,'JC131-275'),(12391,'HG1501-1614.9'),
 (52569,'HV6499-6535'),(5106,'HF5801-6182'),(13363,'GR265-345'),
 (11761,'HX1-79'),(12893,'JX1404-1598'),(12893,'JX220-1195'),
 (12893,'JZ1464-2060'),(11604,'JN3201-4980'),(5471,'HV835-941'),
 (12758,'HG4501-4528.9'),(52567,'HV6435-6492');

DELETE FROM lcc_range WHERE concept_id IN
 (11824,12528,12391,52569,5106,13363,11761,12893,11604,5471,12758,52567);

CREATE TEMP TABLE _candidate_split(
 label TEXT NOT NULL,parent_id INTEGER NOT NULL,start_code TEXT NOT NULL,end_code TEXT NOT NULL
);
INSERT INTO _candidate_split VALUES
 ('U.S. Labor Markets',11824,'HD5723','HD5726'),
 ('Canadian Labor Markets',11824,'HD5727','HD5729.5'),
 ('Labor Markets by Other Country',11824,'HD5730','HD5850.9'),

 ('Modern State',12528,'JC131','JC250'),
 ('Twentieth-Century Political Thought',12528,'JC251','JC273.2'),
 ('Twenty-First-Century Political Thought',12528,'JC274','JC275'),

 ('Banking Periodicals',12391,'HG1501','HG1505'),
 ('Banking Societies',12391,'HG1507','HG1515'),
 ('History of Banking',12391,'HG1551','HG1573'),
 ('Banking Statistics',12391,'HG1576','HG1577.2'),
 ('Banking Reference Works',12391,'HG1600.2','HG1607'),

 ('History of Homicide',52569,'HV6505','HV6513'),
 ('Homicide by Region or Country',52569,'HV6518','HV6535'),

 ('Advertising Directories',5106,'HF5804','HF5808'),
 ('Advertising Methods',5106,'HF5837','HF6146'),
 ('Advertising Agencies',5106,'HF6178','HF6182'),

 ('Middle Eastern Folklore',13363,'GR270','GR295'),
 ('Central Asian Folklore',13363,'GR300','GR300.72'),
 ('South Asian Folklore',13363,'GR302','GR307'),
 ('Southeast Asian Folklore',13363,'GR308','GR326.2'),
 ('East Asian Folklore',13363,'GR330','GR342.5'),

 ('Socialism & Communism Periodicals',11761,'HX1','HX9'),
 ('History of Socialism & Communism',11761,'HX21','HX54.24'),
 ('Scientific Socialism & Communism',11761,'HX56','HX63'),
 ('Socialism & Communism Reference Works',11761,'HX71','HX73'),

 ('Historical Foreign Relations by Country',12893,'JX1404','JX1598'),
 ('Historical International Relations Collections',12893,'JX220','JX1195'),
 ('International Relations in the Americas',12893,'JZ1464','JZ1562'),
 ('International Relations in Europe',12893,'JZ1570','JZ1668'),
 ('International Relations in Asia',12893,'JZ1670','JZ1771'),
 ('International Relations in Africa',12893,'JZ1773','JZ1950'),
 ('International Relations in the Pacific',12893,'JZ1980','JZ2059'),

 ('History of German Government',11604,'JN3241','JN3356.9'),
 ('German Government, 1867–1918',11604,'JN3357','JN3444'),
 ('German Public Administration',11604,'JN3445','JN3753'),
 ('German Political Rights & Participation',11604,'JN3770','JN3901'),
 ('German Political Parties',11604,'JN3925','JN3946'),
 ('German Government, 1918–1945',11604,'JN3951','JN3970'),
 ('Government of German States',11604,'JN4000','JN4980'),

 ('Foundlings',5471,'HV835','HV847'),
 ('Child Day Care',5471,'HV851','HV861'),
 ('Residential Child Care',5471,'HV862','HV866'),
 ('Neglected & Abandoned Children',5471,'HV873','HV887'),
 ('Children with Disabilities — Welfare',5471,'HV888','HV907'),
 ('Fresh-Air Funds',5471,'HV931','HV941'),

 ('Investment Periodicals',12758,'HG4501','HG4503'),
 ('Investment Theory & Methods',12758,'HG4515','HG4515.5'),

 ('Illegal Organizations',52567,'HV6437','HV6453'),
 ('Lynching',52567,'HV6455','HV6471'),
 ('Riots & Unlawful Assemblies',52567,'HV6474','HV6485'),
 ('Disorderly Conduct',52567,'HV6486','HV6491');

INSERT INTO concept(preferred_label)
SELECT DISTINCT s.label FROM _candidate_split s
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);

INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,0
FROM _candidate_split s JOIN concept c ON c.preferred_label=s.label;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id
FROM _candidate_split s JOIN concept c ON c.preferred_label=s.label;

DROP TABLE _candidate_split;
COMMIT;
