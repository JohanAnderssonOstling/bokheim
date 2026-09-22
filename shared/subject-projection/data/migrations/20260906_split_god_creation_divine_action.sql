BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'bokheim:christian-theology:god-creation';

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:christian-theology:doctrine-of-god', 'Doctrine of God'),
  ('bokheim:christian-theology:trinity', 'The Trinity'),
  ('bokheim:christian-theology:holy-spirit', 'The Holy Spirit'),
  ('bokheim:christian-theology:revelation', 'Revelation & Divine Manifestation'),
  ('bokheim:christian-theology:divine-attributes', 'Divine Attributes'),
  ('bokheim:christian-theology:god-humanity', 'Covenants, Theodicy & Relationship with God'),
  ('bokheim:christian-theology:creation', 'Creation, Life & Ecological Theology'),
  ('bokheim:christian-theology:theological-anthropology', 'Theological Anthropology'),
  ('bokheim:christian-theology:animals-creation', 'Animals & Ethics of Creation'),
  ('bokheim:christian-theology:process-theology', 'Process Theology');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:christian-theology:doctrine-of-god', 'bokheim:christian-theology:god-creation', 0, 10),
  ('bokheim:christian-theology:trinity', 'bokheim:christian-theology:god-creation', 0, 20),
  ('bokheim:christian-theology:holy-spirit', 'bokheim:christian-theology:god-creation', 0, 30),
  ('bokheim:christian-theology:revelation', 'bokheim:christian-theology:god-creation', 0, 40),
  ('bokheim:christian-theology:divine-attributes', 'bokheim:christian-theology:god-creation', 0, 50),
  ('bokheim:christian-theology:god-humanity', 'bokheim:christian-theology:god-creation', 0, 60),
  ('bokheim:christian-theology:creation', 'bokheim:christian-theology:god-creation', 0, 70),
  ('bokheim:christian-theology:theological-anthropology', 'bokheim:christian-theology:god-creation', 0, 80),
  ('bokheim:christian-theology:animals-creation', 'bokheim:christian-theology:god-creation', 0, 90),
  ('bokheim:christian-theology:process-theology', 'bokheim:christian-theology:god-creation', 0, 100);

-- The LCC ranges are a non-overlapping partition of the two broad source
-- blocks previously attached to the combined leaf (BT98-180 and BT695-749).
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:christian-theology:doctrine-of-god', 'lcc', 'BT98..BT108.9@1', 0),
  ('bokheim:christian-theology:trinity', 'lcc', 'BT109..BT116.9@1', 0),
  ('bokheim:christian-theology:holy-spirit', 'lcc', 'BT117..BT125.9@1', 0),
  ('bokheim:christian-theology:revelation', 'lcc', 'BT126..BT129.9@1', 0),
  ('bokheim:christian-theology:divine-attributes', 'lcc', 'BT130..BT153.Z@1', 0),
  ('bokheim:christian-theology:god-humanity', 'lcc', 'BT154..BT180.Z@1', 0),
  ('bokheim:christian-theology:creation', 'lcc', 'BT695..BT699.9@1', 0),
  ('bokheim:christian-theology:theological-anthropology', 'lcc', 'BT700..BT745.Z@1', 0),
  ('bokheim:christian-theology:animals-creation', 'lcc', 'BT746..BT749.Z@1', 0),
  ('bokheim:christian-theology:process-theology', 'bisac', 'REL067130', 0);

COMMIT;
