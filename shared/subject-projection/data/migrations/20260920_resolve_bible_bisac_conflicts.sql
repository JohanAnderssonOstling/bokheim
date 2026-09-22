-- Complete the selector moves from 20260908_add_approved_bible_groups_with_selectors.sql.
-- That migration added the specific destinations but retained these broad owners.
-- Keep commentary/studies codes on their approved Bible-part groups, and
-- catechisms on Creeds & Catechisms. Do not change subject ancestry or LCC ranges.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE _bisac_moves(code TEXT PRIMARY KEY, old_id INTEGER, new_id INTEGER);
INSERT INTO _bisac_moves VALUES
 ('REL006790',12375,54049),
 ('REL006800',12375,54047),
 ('REL006810',12375,54048),
 ('REL006820',12375,54048),
 ('REL006880',12372,54049),
 ('REL006890',12373,54049),
 ('REL009000',5800,54050);
-- Fail atomically if a specific destination is absent; rerunning is harmless.
CREATE TEMP TABLE _bisac_guard(ok INTEGER CHECK(ok=1));
INSERT INTO _bisac_guard
SELECT NOT EXISTS (
 SELECT 1 FROM _bisac_moves m WHERE NOT EXISTS (
  SELECT 1 FROM bisac_selector s WHERE s.selector=m.code AND s.concept_id=m.new_id
 )
);
DELETE FROM bisac_selector
WHERE (selector,concept_id) IN (SELECT code,old_id FROM _bisac_moves);
DROP TABLE _bisac_guard;
DROP TABLE _bisac_moves;
COMMIT;
