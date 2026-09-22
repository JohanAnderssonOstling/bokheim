-- Science and Medicine already have detailed subclass and general-reference
-- children. Remove class-wide permissive umbrellas; retain printed intervals
-- exactly on their broad parents.

DELETE FROM lcc_range
WHERE concept_id IN (2725, 4779);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
  (2725, 'R-RZ'),
  (2725, 'R129-R130.52'),
  (4779, 'Q-QR'),
  (4779, 'Q1-Q390');
