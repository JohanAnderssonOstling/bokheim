-- Finish the reviewed high-hit large-range candidate list.

-- General literature: share the established film and theatre subjects rather
-- than duplicating them under literary studies.
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (873,4369,1), (907,4369,1);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('PN1600','PN1988.2',4372),
  ('PN1993','PN1999',873),
  ('PN2000','PN3307.2',907);

-- Political theory and methodology not already covered by the narrower JA
-- psychology, culture, sociology, ethics, and quantitative-method children.
INSERT INTO concept(concept_id,preferred_label)
VALUES(53421,'Foundations of Political Science');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(53421,5369,0);
INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES('JA71','JA74.4',53421);

-- Broad Asian range-shaped source values can safely reach the established
-- regional subject, while detailed country ranges continue to win beneath it.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
  (3494,'DS701-799.9'),
  (3494,'DS520-689');

-- Christian history by broad region.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53422,'Christian History in the Americas'),
  (53423,'European Christian History'),
  (53424,'Asian Christian History'),
  (53425,'African Christian History'),
  (53426,'Christian History in Oceania');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53422,4588,0),(53423,4588,0),(53424,4588,0),(53425,4588,0),(53426,4588,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('BR500','BR730.9',53422), ('BR735','BR1050',53423),
  ('BR1060','BR1357',53424), ('BR1359','BR1470',53425),
  ('BR1475','BR1495',53426);

-- High-use WWII residual divisions.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53427,'World War II General Studies'),
  (53428,'World War II Special Topics'),
  (53429,'World War II Personal Narratives');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53427,3695,0),(53428,3695,0),(53429,3695,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('D743','D744.3',53427),
  ('D810','D810.Z',53428),
  ('D811','D811.5',53429);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES('D805','D805.6',10594);

-- Complete the modern German/Austrian philosophy period.
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
VALUES('B3178','B3396',10939);

-- Personnel-management topics encoded by official Cutter selectors.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53430,'Employee Career Development'),
  (53431,'Employment Interviewing'),
  (53432,'Workplace Diversity'),
  (53433,'Employee Motivation'),
  (53434,'Employee Training & Education'),
  (53435,'Employee Coaching'),
  (53436,'Employee Recruitment');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53430,12274,0),(53431,12274,0),(53432,12274,0),(53433,12274,0),
  (53434,12274,0),(53435,12274,0),(53436,12274,0);
INSERT INTO lcc_selector(concept_id,selector) VALUES
  (53430,'HF5549.5.C35'), (53431,'HF5549.5.I6'),
  (53432,'HF5549.5.M5'),  (53433,'HF5549.5.M63'),
  (53434,'HF5549.5.T7'),  (53435,'HF5549.5.C53'),
  (53436,'HF5549.5.R44');

-- Genealogy by broad LCC geographic blocks.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53437,'English Genealogy'), (53438,'Irish Genealogy'),
  (53439,'Central European Genealogy'), (53440,'French Genealogy'),
  (53441,'German Genealogy'), (53442,'Southern European Genealogy'),
  (53443,'Eastern European Genealogy'), (53444,'Nordic Genealogy'),
  (53445,'Iberian Genealogy');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53437,4709,0),(53438,4709,0),(53439,4709,0),(53440,4709,0),
  (53441,4709,0),(53442,4709,0),(53443,4709,0),(53444,4709,0),(53445,4709,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('CS410','CS479',53437), ('CS480','CS499',53438),
  ('CS500','CS569',53439), ('CS580','CS599',53440),
  ('CS610','CS699',53441), ('CS730','CS789',53442),
  ('CS840','CS879.7',53443), ('CS880','CS899',53444),
  ('CS940','CS969',53445);

-- Regional sculpture history has two explicit broad geographic divisions in
-- the published schedule; leave the uncovered remainder at its parent.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (53446,'Sculpture in the Americas'),
  (53447,'European Sculpture');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (53446,6847,0),(53447,6847,0);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES
  ('NB201','NB439',53446), ('NB450','NB955',53447);
