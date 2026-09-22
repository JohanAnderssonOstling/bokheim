-- Consolidate national and historical biography subjects under Biography.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- U.S. biography periods belong below the U.S. Biography subject.
INSERT INTO concept(preferred_label) VALUES ('By Period');
CREATE TEMP TABLE new_biography_period AS SELECT last_insert_rowid() AS concept_id;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT last_insert_rowid(),51001,0;

UPDATE concept
SET preferred_label=CASE concept_id
  WHEN 51136 THEN 'U.S. Collective Biography'
  WHEN 50851 THEN 'African American Biographies'
  ELSE preferred_label END
WHERE concept_id IN (51136,50851);

DELETE FROM concept_parent WHERE concept_id IN
  (51136,50851,51119,51086,51144,51007,51190);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,(SELECT concept_id FROM new_biography_period),
       ROW_NUMBER() OVER (ORDER BY concept_id)-1
FROM concept
WHERE concept_id IN (51119,51086,51144,51007,51190);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (51136,51001,5),(50851,3225,0);

-- Country biography entries from History move into European Biographies.
UPDATE concept SET preferred_label=CASE concept_id
  WHEN 51827 THEN 'Albanian Biographies'
  WHEN 51731 THEN 'Bulgarian Biographies'
  WHEN 51741 THEN 'Croatian Biographies'
  WHEN 51988 THEN 'Kosovo Biographies'
  WHEN 51625 THEN 'Romanian Biographies'
  WHEN 51673 THEN 'Serbian Biographies'
  WHEN 51720 THEN 'Czechoslovak Biographies'
  WHEN 51894 THEN 'Belarusian Biographies'
  WHEN 51840 THEN 'Estonian Biographies'
  WHEN 51849 THEN 'Latvian Biographies'
  WHEN 51858 THEN 'Lithuanian Biographies'
  WHEN 51914 THEN 'Moldovan Biographies'
  WHEN 51759 THEN 'Danish Biographies'
  WHEN 51774 THEN 'Norwegian Biographies'
  WHEN 51903 THEN 'Maltese Biographies'
  WHEN 51789 THEN 'Belgian Biographies'
  WHEN 51607 THEN 'Scottish Biographies'
  WHEN 51815 THEN 'Welsh Biographies'
  ELSE preferred_label END
WHERE concept_id IN
  (51827,51731,51741,51988,51625,51673,51720,51894,51840,51849,51858,51914,
   51759,51774,51903,51789,51607,51815);
DELETE FROM concept_parent WHERE concept_id IN
  (51827,51731,51741,51988,51625,51673,51720,51894,51840,51849,51858,51914,
   51759,51774,51903,51789,51607,51815);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT concept_id,13353,ROW_NUMBER() OVER (ORDER BY preferred_label)-1
FROM concept
WHERE concept_id IN
  (51827,51731,51741,51988,51625,51673,51720,51894,51840,51849,51858,51914,
   51759,51774,51903,51789,51607,51815);

-- Chinese historical biography entries belong with Asian Biographies.
DELETE FROM concept_parent WHERE concept_id IN (52102,52107,52168);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (52102,13354,0),(52107,13354,1),(52168,13354,2);

-- Merge duplicate Greek and Italian historical biography nodes.
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13422,selector FROM lcc_selector WHERE concept_id=52319;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 13422,selector FROM bisac_selector WHERE concept_id=52319;
DELETE FROM concept_parent WHERE concept_id=52319;
DELETE FROM concept WHERE concept_id=52319;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT 13423,selector FROM lcc_selector WHERE concept_id=13765;
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT 13423,selector FROM bisac_selector WHERE concept_id=13765;
DELETE FROM concept_parent WHERE concept_id=13765;
DELETE FROM concept WHERE concept_id=13765;

-- General historical and population biography groupings remain under Biography.
DELETE FROM concept_parent WHERE concept_id IN (9624,7212,9000);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES (9624,3237,0),(7212,3237,1),(9000,3225,1);

COMMIT;
