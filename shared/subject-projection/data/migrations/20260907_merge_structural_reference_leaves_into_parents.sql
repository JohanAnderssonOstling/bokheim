-- Undo structural catch-all leaves introduced solely to make nonleaf parents
-- range-free. Their already-disjoint residual ranges belong directly to the
-- meaningful parent; genuine specific topical children remain unchanged.

CREATE TEMP TABLE merge_reference_leaf(
  concept_id INTEGER PRIMARY KEY,
  parent_concept_id INTEGER NOT NULL
) STRICT;

INSERT INTO merge_reference_leaf(concept_id, parent_concept_id) VALUES
  (53355, 4961),
  (53356, 4922),
  (53357, 4930),
  (53358, 4939),
  (53359, 4923),
  (53362, 5917),
  (53364, 5918),
  (53365, 446),
  (53366, 451),
  (53367, 450),
  (53368, 449),
  (53370, 285),
  (53372, 289),
  (53374, 292),
  (53376, 282),
  (53377, 282),
  (53381, 237),
  (53382, 277),
  (53383, 287),
  (53384, 286),
  (53385, 281),
  (53386, 208),
  (53388, 358),
  (53390, 358),
  (53391, 355),
  (53392, 356),
  (53393, 352),
  (53394, 217),
  (53395, 389),
  (53396, 389),
  (53397, 405);

UPDATE lcc_range
SET concept_id = (
  SELECT m.parent_concept_id
  FROM merge_reference_leaf m
  WHERE m.concept_id = lcc_range.concept_id
)
WHERE concept_id IN (SELECT concept_id FROM merge_reference_leaf);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
SELECT m.parent_concept_id, s.selector
FROM lcc_selector s
JOIN merge_reference_leaf m ON m.concept_id = s.concept_id;

DELETE FROM lcc_selector
WHERE concept_id IN (SELECT concept_id FROM merge_reference_leaf);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM merge_reference_leaf);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM merge_reference_leaf);

DROP TABLE merge_reference_leaf;
