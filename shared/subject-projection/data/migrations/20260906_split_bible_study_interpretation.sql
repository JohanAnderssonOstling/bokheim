BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'bokheim:bible:study-interpretation';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:bible:study-general', 'General Bible Study'),
  ('bokheim:bible:commentary', 'Biblical Commentary'),
  ('bokheim:bible:criticism-hermeneutics', 'Criticism, Exegesis & Hermeneutics'),
  ('bokheim:bible:literature', 'The Bible as Literature'),
  ('bokheim:bible:stories-retellings', 'Bible Stories & Retellings'),
  ('bokheim:bible:people', 'People of the Bible'),
  ('bokheim:bible:teaching', 'Bible Study & Teaching'),
  ('bokheim:bible:prophecy', 'Biblical Prophecy'),
  ('bokheim:bible:science', 'The Bible & Science'),
  ('bokheim:bible:society', 'The Bible & Society');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:bible:study-general', 'bokheim:bible:study-interpretation', 0, 10),
  ('bokheim:bible:commentary', 'bokheim:bible:study-interpretation', 0, 20),
  ('bokheim:bible:criticism-hermeneutics', 'bokheim:bible:study-interpretation', 0, 30),
  ('bokheim:bible:literature', 'bokheim:bible:study-interpretation', 0, 40),
  ('bokheim:bible:stories-retellings', 'bokheim:bible:study-interpretation', 0, 50),
  ('bokheim:bible:people', 'bokheim:bible:study-interpretation', 0, 60),
  ('bokheim:bible:teaching', 'bokheim:bible:study-interpretation', 0, 70),
  ('bokheim:bible:prophecy', 'bokheim:bible:study-interpretation', 0, 80),
  ('bokheim:bible:science', 'bokheim:bible:study-interpretation', 0, 90),
  ('bokheim:bible:society', 'bokheim:bible:study-interpretation', 0, 100);

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:bible:study-general', 'bisac', 'REL006000', 0),
  ('bokheim:bible:study-general', 'bisac', 'REL006630', 1),
  ('bokheim:bible:study-general', 'bisac', 'REL006700', 2),
  ('bokheim:bible:study-general', 'lcc', 'BS410..BS499.9@1', 0),

  ('bokheim:bible:commentary', 'bisac', 'REL006050', 0),
  ('bokheim:bible:commentary', 'bisac', 'REL006060', 1),
  ('bokheim:bible:commentary', 'bisac', 'REL006070', 2),
  ('bokheim:bible:commentary', 'bisac', 'REL006750', 3),
  ('bokheim:bible:commentary', 'bisac', 'REL006760', 4),
  ('bokheim:bible:commentary', 'bisac', 'REL006770', 5),
  ('bokheim:bible:commentary', 'bisac', 'REL006780', 6),
  ('bokheim:bible:commentary', 'bisac', 'REL006790', 7),
  ('bokheim:bible:commentary', 'bisac', 'REL006800', 8),
  ('bokheim:bible:commentary', 'bisac', 'REL006810', 9),
  ('bokheim:bible:commentary', 'bisac', 'REL006820', 10),
  ('bokheim:bible:commentary', 'bisac', 'REL006830', 11),

  ('bokheim:bible:criticism-hermeneutics', 'bisac', 'REL006080', 0),
  ('bokheim:bible:criticism-hermeneutics', 'bisac', 'REL006090', 1),
  ('bokheim:bible:criticism-hermeneutics', 'bisac', 'REL006100', 2),
  ('bokheim:bible:criticism-hermeneutics', 'bisac', 'REL006400', 3),
  ('bokheim:bible:criticism-hermeneutics', 'lcc', 'BS500..BS534.8@1', 0),
  ('bokheim:bible:criticism-hermeneutics', 'lcc', 'BS1160..BS1191.5@1', 1),
  ('bokheim:bible:literature', 'lcc', 'BS535..BS545.9@1', 0),
  ('bokheim:bible:stories-retellings', 'lcc', 'BS546..BS568.9@1', 0),
  ('bokheim:bible:people', 'lcc', 'BS569..BS584.81@1', 0),
  ('bokheim:bible:teaching', 'lcc', 'BS584.82..BS646.9@1', 0),
  ('bokheim:bible:prophecy', 'bisac', 'REL006140', 0),
  ('bokheim:bible:prophecy', 'lcc', 'BS647..BS649@1', 0),
  ('bokheim:bible:science', 'lcc', 'BS650..BS669.9@1', 0),
  ('bokheim:bible:society', 'lcc', 'BS670..BS680@1', 0);

COMMIT;
