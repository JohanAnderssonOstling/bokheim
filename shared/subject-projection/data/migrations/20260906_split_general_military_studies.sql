BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE concept_id = 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1'
   OR concept_id LIKE 'bokheim:general-military:%';

-- Retain the exact class-level notation, but remove the regressed U* wildcard.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 'lcc', 'U', 0);

INSERT OR IGNORE INTO concept (concept_id, preferred_label) VALUES
  ('bokheim:general-military:reference', 'Military Reference & General Collections'),
  ('bokheim:general-military:war-society', 'War, Philosophy & Military Society'),
  ('bokheim:general-military:history-biography', 'Military History & Biography'),
  ('bokheim:general-military:handbooks-planning', 'Military Handbooks & Planning'),
  ('bokheim:general-military:strategy-tactics', 'Strategy & Tactics'),
  ('bokheim:general-military:field-operations', 'Logistics & Field Operations'),
  ('bokheim:general-military:special-warfare', 'Special Warfare & Peacekeeping'),
  ('bokheim:general-military:maneuvers-games', 'Maneuvers, Training Grounds & War Games'),
  ('bokheim:general-military:service-research', 'Military Service, Ceremonies & Research'),
  ('bokheim:general-military:education', 'Military Education & Training'),
  ('bokheim:general-military:war-observation', 'Military Studies of Particular Wars'),
  ('bokheim:general-military:life-customs', 'Military Life & Customs'),
  ('bokheim:general-military:arms-armor', 'Arms, Armor & Military Weapons'),
  ('bokheim:general-military:nonmilitary-drill', 'Nonmilitary Drill');

INSERT OR IGNORE INTO concept_parent
  (concept_id, parent_concept_id, ordinal, display_order) VALUES
  ('bokheim:general-military:reference', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 10),
  ('bokheim:general-military:war-society', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 20),
  ('bokheim:general-military:history-biography', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 30),
  ('bokheim:general-military:handbooks-planning', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 40),
  ('bokheim:general-military:strategy-tactics', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 50),
  ('bokheim:general-military:field-operations', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 60),
  ('bokheim:general-military:special-warfare', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 70),
  ('bokheim:general-military:maneuvers-games', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 80),
  ('bokheim:general-military:service-research', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 90),
  ('bokheim:general-military:education', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 100),
  ('bokheim:general-military:war-observation', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 110),
  ('bokheim:general-military:life-customs', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 120),
  ('bokheim:general-military:arms-armor', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 130),
  ('bokheim:general-military:nonmilitary-drill', 'unified:e107a1fd-10e1-5935-8eed-d460c98629d1', 0, 140);

-- Broad ranges are intentional on these curated leaves and form a complete,
-- mutually exclusive partition of the official U1-U900 class.
INSERT INTO source_selector (concept_id, system_id, selector, ordinal) VALUES
  ('bokheim:general-military:reference', 'lcc', 'U1..U20.9@1', 0),
  ('bokheim:general-military:war-society', 'lcc', 'U21..U26.9@1', 0),
  ('bokheim:general-military:history-biography', 'lcc', 'U27..U59.9@1', 0),
  ('bokheim:general-military:handbooks-planning', 'lcc', 'U60..U160.9@1', 0),
  ('bokheim:general-military:strategy-tactics', 'lcc', 'U161..U167.9@1', 0),
  ('bokheim:general-military:field-operations', 'lcc', 'U168..U239.9@1', 0),
  ('bokheim:general-military:special-warfare', 'lcc', 'U240..U279.9@1', 0),
  ('bokheim:general-military:maneuvers-games', 'lcc', 'U280..U319.9@1', 0),
  ('bokheim:general-military:service-research', 'lcc', 'U320..U399.9@1', 0),
  ('bokheim:general-military:education', 'lcc', 'U400..U718.9@1', 0),
  ('bokheim:general-military:war-observation', 'lcc', 'U719..U749.9@1', 0),
  ('bokheim:general-military:life-customs', 'lcc', 'U750..U789.9@1', 0),
  ('bokheim:general-military:arms-armor', 'lcc', 'U790..U899.9@1', 0),
  ('bokheim:general-military:nonmilitary-drill', 'lcc', 'U900..U900.Z@1', 0);

COMMIT;
