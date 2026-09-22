BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50884,'Indigenous History of Canada'),
  (50885,'Indigenous History of the New Southwest'),
  (50886,'Great Plains Indigenous History'),
  (50887,'Indigenous History of California'),
  (50888,'Indigenous History of the Northwest Coast'),
  (50889,'Indigenous History of Illinois'),
  (50890,'Indigenous History of the Southern States'),
  (50891,'Indigenous History of British Columbia'),
  (50892,'Indigenous History of Arizona'),
  (50893,'Indigenous History of the Eastern Woodlands');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50884,3398,0),
  (50885,3398,0),
  (50886,3398,0),
  (50887,3398,0),
  (50888,3398,0),
  (50889,3398,0),
  (50890,3398,0),
  (50891,3398,0),
  (50892,3398,0),
  (50893,3398,0);

-- Exact geographic Cutters from the E78 schedule. Adjacent values name
-- different states, provinces, or regions and must not be inferred as ranges.
INSERT INTO source_selector(concept_id,system_id,selector) VALUES
  (50884,'lcc','E78.C2'),
  (50885,'lcc','E78.S7'),
  (50886,'lcc','E78.G73'),
  (50887,'lcc','E78.C15'),
  (50888,'lcc','E78.N78'),
  (50889,'lcc','E78.I3'),
  (50890,'lcc','E78.S65'),
  (50891,'lcc','E78.B9'),
  (50892,'lcc','E78.A7'),
  (50893,'lcc','E78.E2');

COMMIT;
