-- Dissolve General Science and its redundant Physical, Earth/Space, and
-- Life/Cognitive/Applied wrappers. Retain their useful subjects under existing
-- canonical disciplines; promote the four distinct cross-disciplinary groups.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE science_move(
    concept_id TEXT PRIMARY KEY,
    parent_concept_id TEXT NOT NULL,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;

-- Distinct cross-disciplinary groups move directly under Science & Nature.
INSERT INTO science_move
SELECT child.concept_id, 'unified:2d01b451-4e64-5f6b-a068-a6f7ea63e386', route.display_order
FROM concept_parent route
JOIN concept child ON child.concept_id = route.concept_id
WHERE route.parent_concept_id = 'unified:85bd0cd4-d369-53a4-a32e-749e0eeb3201'
  AND child.preferred_label IN (
      'Systems, Complexity & Computation',
      'Methods, Instruments & Measurement',
      'History, Philosophy & Society',
      'Education, Organizations & Reference'
  );

-- Physical-science subjects move to Physics, except Chemistry itself.
INSERT INTO science_move
SELECT child.concept_id,
       CASE child.preferred_label
           WHEN 'Chemistry' THEN 'unified:2d01b451-4e64-5f6b-a068-a6f7ea63e386'
           ELSE 'unified:303e0e77-153f-5abb-bd90-88b5155a4ed0'
       END,
       route.display_order
FROM concept wrapper
JOIN concept_parent route ON route.parent_concept_id = wrapper.concept_id
JOIN concept child ON child.concept_id = route.concept_id
WHERE wrapper.concept_id = 'bokheim:science:physical';

-- Earth and environmental subjects move to the established earth branch;
-- Space Science moves to Astronomy & Space Science.
INSERT INTO science_move
SELECT child.concept_id,
       CASE child.preferred_label
           WHEN 'Space Science' THEN 'unified:63c2b0cd-d97a-5db2-b7d5-e73eed42afec'
           ELSE 'unified:56de5b97-c160-57a3-b501-29928c3fee60'
       END,
       route.display_order
FROM concept wrapper
JOIN concept_parent route ON route.parent_concept_id = wrapper.concept_id
JOIN concept child ON child.concept_id = route.concept_id
WHERE wrapper.concept_id = 'bokheim:science:earth-space-environment';

-- Life-science subjects move to Biology; Cognitive Science to Psychology;
-- Natural History to its existing branch. The generic Applied Sciences marker
-- is absorbed into the Applied Sciences & Technology root below.
INSERT INTO science_move
SELECT child.concept_id,
       CASE child.preferred_label
           WHEN 'Cognitive Science' THEN 'unified:91e9ffa6-f917-5bc8-83c8-c0c5933abcd3'
           WHEN 'Natural History' THEN 'unified:6ff9be52-aff0-5e20-948a-a814cf3df5b1'
           ELSE 'unified:ba1ce9c4-e1f0-5660-829a-d836b81a894b'
       END,
       route.display_order
FROM concept wrapper
JOIN concept_parent route ON route.parent_concept_id = wrapper.concept_id
JOIN concept child ON child.concept_id = route.concept_id
WHERE wrapper.concept_id = 'bokheim:science:life-cognitive-applied'
  AND child.preferred_label <> 'Applied Sciences';

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT move.concept_id, move.parent_concept_id,
       COALESCE((SELECT max(existing.ordinal) + 1 FROM concept_parent existing
                 WHERE existing.concept_id = move.concept_id), 0),
       move.display_order
FROM science_move move
WHERE move.concept_id <> move.parent_concept_id
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent existing
      WHERE existing.concept_id = move.concept_id
        AND existing.parent_concept_id = move.parent_concept_id
  );

-- Preserve the generic Applied Sciences selector on its canonical root.
INSERT INTO source_label(concept_id, label, ordinal)
SELECT 'unified:22ed848f-f758-5528-a501-4cca0722a7cf', label.label,
       COALESCE((SELECT max(existing.ordinal) + 1 FROM source_label existing
                 WHERE existing.concept_id='unified:22ed848f-f758-5528-a501-4cca0722a7cf'),0)
FROM source_label label
WHERE label.concept_id='unified:c602532a-2a20-5d6e-8697-2ae4aa7a094e'
  AND NOT EXISTS (SELECT 1 FROM source_label existing
                  WHERE existing.concept_id='unified:22ed848f-f758-5528-a501-4cca0722a7cf'
                    AND existing.label=label.label);
INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT 'unified:22ed848f-f758-5528-a501-4cca0722a7cf', selector.system_id, selector.selector,
       COALESCE((SELECT max(existing.ordinal) + 1 FROM source_selector existing
                 WHERE existing.concept_id='unified:22ed848f-f758-5528-a501-4cca0722a7cf'
                   AND existing.system_id=selector.system_id),0)
FROM source_selector selector
WHERE selector.concept_id='unified:c602532a-2a20-5d6e-8697-2ae4aa7a094e'
  AND NOT EXISTS (SELECT 1 FROM source_selector existing
                  WHERE existing.concept_id='unified:22ed848f-f758-5528-a501-4cca0722a7cf'
                    AND existing.system_id=selector.system_id
                    AND existing.selector=selector.selector);

CREATE TEMP TABLE obsolete_science_wrapper(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO obsolete_science_wrapper VALUES
('unified:85bd0cd4-d369-53a4-a32e-749e0eeb3201'),
('bokheim:science:physical'),
('bokheim:science:earth-space-environment'),
('bokheim:science:life-cognitive-applied'),
('unified:c602532a-2a20-5d6e-8697-2ae4aa7a094e');

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM obsolete_science_wrapper)
   OR parent_concept_id IN (SELECT concept_id FROM obsolete_science_wrapper);
DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM obsolete_science_wrapper)
  AND NOT EXISTS (SELECT 1 FROM concept_parent p
                  WHERE p.concept_id=concept.concept_id OR p.parent_concept_id=concept.concept_id);

DROP TABLE obsolete_science_wrapper;
DROP TABLE science_move;
COMMIT;
