-- Unify literature's language/tradition and regional views. Broad LCC
-- classes stay on the short tradition groups; specific BISAC headings remain
-- on country, regional, and fiction subjects beneath them.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

UPDATE concept SET preferred_label='Language & Tradition' WHERE concept_id=11224;
UPDATE concept SET preferred_label='English' WHERE concept_id=5585;
UPDATE concept SET preferred_label='East Asian & Pacific' WHERE concept_id=5590;
UPDATE concept SET preferred_label='African & Middle Eastern' WHERE concept_id=5591;
UPDATE concept SET preferred_label='American & Caribbean' WHERE concept_id=5727;

INSERT OR IGNORE INTO lcc_selector (concept_id,selector) VALUES
  (5588,'PA'),
  (5585,'PR'),
  (5587,'PD'),(5587,'PT'),
  (5586,'PQ'),
  (5589,'PG'),
  (5590,'PL'),
  (5591,'PJ'),(5591,'PL'),
  (5727,'PS');

INSERT OR IGNORE INTO bisac_selector (concept_id,selector)
VALUES (11224,'FIC099000');

-- Specific European BISAC subjects move below their LCC-aligned traditions.
DELETE FROM concept_parent
WHERE concept_id IN (4159,4173,4174,4175,4183,4184,4189,4192,4193,4198,
                     4202,4211,4214,4215,4216,4220,4226,4230,4231,4233,4470)
   OR parent_concept_id IN (4182,11223);

INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal) VALUES
  (4192,5588,0),
  (4175,5585,0),
  (4159,5587,0),(4174,5587,0),(4209,5587,0),(4183,5587,0),
  (4189,5587,0),(4198,5587,0),(4211,5587,0),(4220,5587,0),(4230,5587,0),(4233,5587,0),
  (4173,5589,0),(4193,5589,0),(4214,5589,0),(4216,5589,0),
  (4184,5586,0),(4202,5586,0),(4215,5586,0),(4226,5586,0),(4470,5586,0);

-- The remaining BISAC regional groups become short tradition groups under
-- Language & Tradition rather than a separate Literature by Region branch.
DELETE FROM concept_parent
WHERE concept_id IN (5590,5591,5727)
   OR parent_concept_id=11223;
INSERT INTO concept_parent (concept_id,parent_concept_id,ordinal) VALUES
  (5590,11224,0),(5591,11224,0),(5727,11224,0);

DELETE FROM bisac_selector WHERE concept_id=4182;
DELETE FROM lcc_selector WHERE concept_id=4182;
DELETE FROM ddc_selector WHERE concept_id=4182;
DELETE FROM source_label WHERE concept_id IN (4182,11223);
DELETE FROM concept_parent WHERE concept_id IN (4182,11223) OR parent_concept_id IN (4182,11223);
DELETE FROM concept WHERE concept_id IN (4182,11223);

COMMIT;
