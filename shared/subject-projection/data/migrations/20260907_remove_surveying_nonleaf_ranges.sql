-- Surveying's residual official bands are too sparsely used to justify new
-- leaves. Preserve their printed spans and endpoint headings exactly instead.

DELETE FROM lcc_range WHERE concept_id=293;

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES
  (293,'TA501-625'),
  (293,'TA501'),
  (293,'TA514.7-531'),
  (293,'TA514.7'),
  (293,'TA531'),
  (293,'TA532-533'),
  (293,'TA532'),
  (293,'TA533'),
  (293,'TA535-538'),
  (293,'TA535'),
  (293,'TA538'),
  (293,'TA544-545'),
  (293,'TA544'),
  (293,'TA545'),
  (293,'TA590'),
  (293,'TA595-595.5'),
  (293,'TA595'),
  (293,'TA595.5'),
  (293,'TA606-610'),
  (293,'TA606'),
  (293,'TA610'),
  (293,'TA613-614'),
  (293,'TA613'),
  (293,'TA614');
