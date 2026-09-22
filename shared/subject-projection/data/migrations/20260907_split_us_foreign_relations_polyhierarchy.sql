BEGIN;

-- Regional browse structure on the U.S. side.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (51020,'Americas'),(51021,'Europe'),(51022,'Asia'),(51023,'Middle East');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51020,3410,0),(51021,3410,1),(51022,3410,2),(51023,3410,3);

-- Missing foreign-relations branches on the counterpart-country side.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (51024,'Chinese Foreign Relations History'),
  (51025,'Japanese Foreign Relations History'),
  (51026,'Canadian Foreign Relations History'),
  (51027,'Cuban Foreign Relations History'),
  (51028,'Iranian Foreign Relations History'),
  (51029,'German Foreign Relations History'),
  (51030,'British Foreign Relations History'),
  (51031,'French Foreign Relations History'),
  (51032,'Russian Foreign Relations History');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51024,3493,0),(51025,3498,0),(51026,3400,0),(51027,3380,0),
  (51028,3636,0),(51029,3524,0),(51030,3521,0),(51031,3523,0),
  (51032,3595,0);

-- Shared bilateral concepts: ordinal 0 is the U.S. regional route and
-- ordinal 1 is the counterpart country's foreign-relations route.
INSERT INTO concept(concept_id,preferred_label) VALUES
  (51033,'United States–China Relations'),
  (51034,'United States–Soviet Union Relations'),
  (51035,'United States–Japan Relations'),
  (51036,'United States–Russia Relations'),
  (51037,'United States–Great Britain Relations'),
  (51038,'United States–Cuba Relations'),
  (51039,'United States–Iran Relations'),
  (51040,'United States–Germany Relations'),
  (51041,'United States–Korea Relations'),
  (51042,'United States–Canada Relations'),
  (51043,'United States–France Relations');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51033,51022,0),(51033,51024,1),
  (51034,51021,0),(51034,12629,1),
  (51035,51022,0),(51035,51025,1),
  (51036,51021,0),(51036,51032,1),
  (51037,51021,0),(51037,51030,1),
  (51038,51020,0),(51038,51027,1),
  (51039,51023,0),(51039,51028,1),
  (51040,51021,0),(51040,51029,1),
  (51041,51022,0),(51041,10441,1),
  (51042,51020,0),(51042,51026,1),
  (51043,51021,0),(51043,51031,1);

INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (51033,'lcc','E183.8.C5'),(51033,'lcc','E183.8.C6'),
  (51034,'lcc','E183.8.S65'),
  (51035,'lcc','E183.8.J3'),
  (51036,'lcc','E183.8.R9'),
  (51037,'lcc','E183.8.G7'),
  (51038,'lcc','E183.8.C9'),
  (51039,'lcc','E183.8.I55'),
  (51040,'lcc','E183.8.G3'),
  (51041,'lcc','E183.8.K6'),
  (51042,'lcc','E183.8.C2'),
  (51043,'lcc','E183.8.F8');

COMMIT;
