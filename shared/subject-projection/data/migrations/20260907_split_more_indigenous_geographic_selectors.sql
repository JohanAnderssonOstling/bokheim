BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50894,'Indigenous History of the American West'),
  (50895,'Indigenous History of Québec'),
  (50896,'Indigenous History of Texas'),
  (50897,'Indigenous History of Florida'),
  (50898,'Indigenous History of Alaska'),
  (50899,'Indigenous History of Ontario'),
  (50900,'Indigenous History of New Mexico'),
  (50901,'Indigenous History of Washington State'),
  (50902,'Indigenous History of New England'),
  (50903,'Indigenous History of the Pacific Northwest');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50894,3398,0),
  (50895,3398,0),
  (50896,3398,0),
  (50897,3398,0),
  (50898,3398,0),
  (50899,3398,0),
  (50900,3398,0),
  (50901,3398,0),
  (50902,3398,0),
  (50903,3398,0);

INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (50894,'lcc','E78.W5'),
  (50895,'lcc','E78.Q3'),
  (50896,'lcc','E78.T4'),
  (50897,'lcc','E78.F6'),
  (50898,'lcc','E78.A3'),
  (50899,'lcc','E78.O5'),
  (50900,'lcc','E78.N65'),
  (50901,'lcc','E78.W3'),
  (50902,'lcc','E78.N5'),
  (50903,'lcc','E78.N77');

COMMIT;
