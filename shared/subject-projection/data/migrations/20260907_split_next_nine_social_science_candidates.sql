BEGIN;

CREATE TEMP TABLE _leaf(label TEXT NOT NULL,parent_id INTEGER NOT NULL);
CREATE TEMP TABLE _exact(label TEXT NOT NULL,selector TEXT NOT NULL);
CREATE TEMP TABLE _range(label TEXT NOT NULL,start_code TEXT NOT NULL,end_code TEXT NOT NULL);

INSERT INTO _leaf VALUES
 ('Air-Pollution Economics',5139),
 ('Environmental Policy & Sustainable Development',5139),
 ('Pollution Economics',5139),('Waste Economics',5139),('Water-Pollution Economics',5139),
 ('U.S. Defense Organization',13338),('U.S. Army',13338),('U.S. National Guard & Reserves',13338),
 ('Sociology Periodicals',13838),('Sociology Museums & Exhibitions',13838),
 ('Sociology Directories & Institutions',13838),('Sociology Special Works',13838),
 ('Early Sociological Theory',12260),('Historical Sociology',12260),
 ('Social Elements & Forces',12260),('Modern Sociological Theories',12260),
 ('Visual Sociology',12260),
 ('Basic Needs',5466),('Famine & Food Scarcity',5466),('Economic Poverty',5466),
 ('Domestic Anti-Poverty Programs',5466),
 ('Automation & the Economy',13617),('High-Technology Industries',13617),
 ('Technological Innovation & Transfer',13617),
 ('Culture & Progress',52373),('Cultural Theory',52373),('Heredity & Social Progress',52373),
 ('Computer Crime — General',12731),('U.S. Computer Crime',12731),
 ('Computer Crime by Region or Country',12731);

INSERT INTO _exact VALUES
 ('Air-Pollution Economics','HC79.A4'),
 ('Environmental Policy & Sustainable Development','HC79.E5'),
 ('Pollution Economics','HC79.P55'),('Waste Economics','HC79.W3'),
 ('Water-Pollution Economics','HC79.W32'),
 ('Sociology Periodicals','HM1'),('Sociology Periodicals','HM7'),
 ('Sociology Museums & Exhibitions','HM41'),('Sociology Museums & Exhibitions','HM43'),
 ('Early Sociological Theory','HM24'),('Early Sociological Theory','HM37'),
 ('Historical Sociology','HM104'),('Social Elements & Forces','HM201'),
 ('Visual Sociology','HM500'),
 ('Basic Needs','HC79.B38'),('Famine & Food Scarcity','HC79.F3'),
 ('Economic Poverty','HC79.P6'),('Domestic Anti-Poverty Programs','HC79.P63'),
 ('Automation & the Economy','HC79.A9'),('High-Technology Industries','HC79.H53'),
 ('Technological Innovation & Transfer','HC79.T4'),
 ('Culture & Progress','HM101'),('Cultural Theory','HM108'),
 ('Heredity & Social Progress','HM121'),
 ('Computer Crime — General','HV6773'),('Computer Crime — General','HV6773.15'),
 ('U.S. Computer Crime','HV6773.2');

INSERT INTO _range VALUES
 ('U.S. Defense Organization','UA23.A1','UA23.7'),
 ('U.S. Army','UA24','UA39'),
 ('U.S. National Guard & Reserves','UA42','UA560'),
 ('Sociology Periodicals','HM401','HM417'),
 ('Sociology Directories & Institutions','HM418','HM429'),
 ('Sociology Museums & Exhibitions','HM581','HM582'),
 ('Sociology Special Works','HM583','HM608'),
 ('Modern Sociological Theories','HM479.2','HM499'),
 ('Computer Crime — General','HV6772','HV6773.19'),
 ('U.S. Computer Crime','HV6773.2','HV6773.25'),
 ('Computer Crime by Region or Country','HV6773.26','HV6773.3');

-- Remove the old ownership before assigning the selectors to exclusive leaves.
DELETE FROM lcc_selector WHERE concept_id IN
 (5139,13338,13838,12260,5466,13617,52373,12731);
DELETE FROM lcc_range WHERE concept_id IN
 (13338,13838,12260,12731);

-- Preserve only genuine broad notations as exact parent matches.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (13338,'UA23.A1-UA585'),(13838,'HM401-429'),(13838,'HM581-608'),
 (12260,'HM479.2-499'),(12731,'HV6772-6773.3');

INSERT INTO concept(preferred_label)
SELECT DISTINCT label FROM _leaf s
WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);

-- Existing concepts may already have ordinal zero beneath another parent.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,s.parent_id,
       COALESCE((SELECT max(p.ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
FROM _leaf s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS(SELECT 1 FROM concept_parent p
                 WHERE p.concept_id=c.concept_id AND p.parent_concept_id=s.parent_id);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT c.concept_id,s.selector FROM _exact s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id FROM _range s JOIN concept c ON c.preferred_label=s.label;

-- HN49.C6 is one real LCC topic, not three different selectors.  Keep the
-- subject as a leaf and discard generated suffix/wildcard variants.
DELETE FROM lcc_selector WHERE concept_id=12416 AND selector IN ('HN49.C6+','HN49.C6S558x');

DROP TABLE _range;
DROP TABLE _exact;
DROP TABLE _leaf;
COMMIT;
