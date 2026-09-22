-- Replace the Elementary Mathematics umbrella with its two occupied continuous
-- divisions. The intervening arithmetic headings are isolated exact topics.

INSERT INTO concept(concept_id,preferred_label)
VALUES
  (53352,'Elementary Mathematics Textbooks'),
  (53353,'Arithmetic Education');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES
  (53352,4934,0),
  (53353,4934,0);

DELETE FROM lcc_range WHERE concept_id=4934;

INSERT INTO lcc_range(start_code,end_code,concept_id)
VALUES
  ('QA101','QA107.2',53352),
  ('QA135','QA139',53353);

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (4934,'QA101-145'),
  (4934,'QA119');
