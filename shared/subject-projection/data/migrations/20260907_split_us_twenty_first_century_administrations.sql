BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51082,'George W. Bush Administrations, 2001–2009'),
  (51083,'Obama Administrations, 2009–2017'),
  (51084,'Trump Administration, 2017–2021'),
  (51085,'Biden Administration, 2021–2025');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51082,3406,0),
  (51083,3406,1),
  (51084,3406,2),
  (51085,3406,3);

-- These are the administration spans in the Library of Congress E schedule.
-- General twenty-first-century material stays on the parent. Presidential
-- campaign numbers E905-E906, E910-E911, E915, and E919 are intentionally
-- left unassigned: campaigns are events, not broad historical eras.
DELETE FROM lcc_range
WHERE concept_id=3406
  AND class_letters='E'
  AND start_value='891'
  AND end_value='914';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','891','901.999',3406),
  ('E','902','904.999',51082),
  ('E','907','909.999',51083),
  ('E','912','914.999',51084),
  ('E','916','918.999',51085);

-- Later schedule numbers had previously been attached as exact selectors.
-- The administration numbers now belong to their exclusive ranges; campaign
-- numbers are removed rather than retained on the broad parent.
DELETE FROM lcc_selector
WHERE concept_id=3406
  AND selector IN ('E912','E913','E913.3','E914','E915',
                   'E916','E917','E917.3','E918','E919');

COMMIT;
