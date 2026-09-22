-- Replace exact-code enumerations on leaves with their official same-meaning LCC spans.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE _leaf_span(concept_id INTEGER PRIMARY KEY, selector TEXT NOT NULL);
INSERT INTO _leaf_span VALUES
  (10155,'DA480..DA522'),
  (10213,'DA529..DA565'),
  (12747,'DC611.B841..DC611.B9173'),
  (12751,'DP302.A41..DP302.A55'),
  (12753,'DP302.B16..DP302.B3'),
  (12749,'DP302.C57..DP302.C69'),
  (3549,'DR1932..DR2125'),
  (3871,'PT8100..PT8167'),
  (4395,'PT8800..PT8942'),
  (4474,'PT9725..PT9850'),
  (12712,'PQ6174.95..PQ6215'),
  (12713,'PQ6247..PQ6269');

-- Every target is deliberately a leaf: permissive ranges must not sit on parents.
DELETE FROM _leaf_span
WHERE EXISTS(SELECT 1 FROM concept_parent cp WHERE cp.parent_concept_id=_leaf_span.concept_id);

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN (SELECT concept_id FROM _leaf_span);

INSERT INTO source_selector(concept_id,system_id,selector)
SELECT concept_id,'lcc',selector FROM _leaf_span;

DROP TABLE _leaf_span;
COMMIT;
