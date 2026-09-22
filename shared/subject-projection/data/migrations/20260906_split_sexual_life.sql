BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:sexuality:history-behavior', 'Sexual History, Customs & Behavior'),
  ('bokheim:sexuality:education-ethics', 'Sex Education & Ethics'),
  ('bokheim:sexuality:group-digital', 'Group & Digital Sexuality'),
  ('bokheim:sexuality:orientation-minorities', 'Sexual Orientation & Minorities'),
  ('bokheim:sexuality:paraphilias', 'Paraphilias'),
  ('bokheim:sexuality:prostitution', 'Prostitution'),
  ('bokheim:sexuality:sex-tourism', 'Sex Tourism'),
  ('bokheim:sexuality:masturbation', 'Masturbation'),
  ('bokheim:sexuality:eunuchs', 'Eunuchs & Emasculation');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:sexuality:history-behavior', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 10),
  ('bokheim:sexuality:education-ethics', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 20),
  ('bokheim:sexuality:group-digital', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 30),
  ('bokheim:sexuality:orientation-minorities', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 40),
  ('bokheim:sexuality:paraphilias', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 50),
  ('bokheim:sexuality:prostitution', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 60),
  ('bokheim:sexuality:sex-tourism', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 70),
  ('bokheim:sexuality:masturbation', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 80),
  ('bokheim:sexuality:eunuchs', 'unified:9a0e7e96-c0c1-5697-ab78-47c538838e56', 0, 90);

-- These leaf ranges partition the former HQ12..HQ449 coverage.  The small
-- numeric gaps are retained inside the nearest meaningful LCC division so
-- that valid, less-common classifications are not silently discarded.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:sexuality:history-behavior', 'lcc', 'HQ12..HQ30.7@1', 0),
  ('bokheim:sexuality:education-ethics', 'lcc', 'HQ31..HQ65.9@1', 0),
  ('bokheim:sexuality:group-digital', 'lcc', 'HQ66..HQ67.9@1', 0),
  ('bokheim:sexuality:paraphilias', 'lcc', 'HQ68..HQ72.7@1', 0),
  ('bokheim:sexuality:orientation-minorities', 'lcc', 'HQ72.8..HQ78.9@1', 0),
  ('bokheim:sexuality:paraphilias', 'lcc', 'HQ79..HQ100.9@1', 1),
  ('bokheim:sexuality:prostitution', 'lcc', 'HQ101..HQ443.9@1', 0),
  ('bokheim:sexuality:sex-tourism', 'lcc', 'HQ444..HQ445.9@1', 0),
  ('bokheim:sexuality:masturbation', 'lcc', 'HQ446..HQ448.9@1', 0),
  ('bokheim:sexuality:eunuchs', 'lcc', 'HQ449..HQ449.Z@1', 0);

COMMIT;
