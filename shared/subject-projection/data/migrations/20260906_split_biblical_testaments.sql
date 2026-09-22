BEGIN IMMEDIATE;

UPDATE concept SET preferred_label = 'Old & New Testaments'
WHERE concept_id = 'bokheim:bible:texts-canon';
DELETE FROM source_selector
WHERE concept_id = 'bokheim:bible:texts-canon';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:bible:old-testament', 'Old Testament'),
  ('bokheim:bible:new-testament', 'New Testament');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:bible:old-testament', 'bokheim:bible:texts-canon', 0, 10),
  ('bokheim:bible:new-testament', 'bokheim:bible:texts-canon', 0, 20);

DELETE FROM source_selector
WHERE concept_id = 'bokheim:bible:study-interpretation'
  AND system_id = 'bisac'
  AND selector IN (
    'REL006210','REL006840','REL006850','REL006740','REL006730','REL006880',
    'REL006220','REL006710','REL006720','REL006860','REL006870','REL006890'
  );

INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:bible:old-testament', 'bisac', 'REL006210', 0),
  ('bokheim:bible:old-testament', 'bisac', 'REL006840', 1),
  ('bokheim:bible:old-testament', 'bisac', 'REL006850', 2),
  ('bokheim:bible:old-testament', 'bisac', 'REL006740', 3),
  ('bokheim:bible:old-testament', 'bisac', 'REL006730', 4),
  ('bokheim:bible:old-testament', 'bisac', 'REL006880', 5),
  ('bokheim:bible:old-testament', 'lcc', 'BS701..BS1830@1', 0),
  ('bokheim:bible:new-testament', 'bisac', 'REL006220', 0),
  ('bokheim:bible:new-testament', 'bisac', 'REL006710', 1),
  ('bokheim:bible:new-testament', 'bisac', 'REL006720', 2),
  ('bokheim:bible:new-testament', 'bisac', 'REL006860', 3),
  ('bokheim:bible:new-testament', 'bisac', 'REL006870', 4),
  ('bokheim:bible:new-testament', 'bisac', 'REL006890', 5),
  ('bokheim:bible:new-testament', 'lcc', 'BS1901..BS2970@1', 0);

COMMIT;
