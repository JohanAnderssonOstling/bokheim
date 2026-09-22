-- Replace the editorial Mind, Brain & Behavior and Cognitive Psychology &
-- Cognition wrappers with direct, recognizable Psychology subjects. Promoted
-- subjects are terminal leaves in the curated taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE psychology_context(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO psychology_context
SELECT psychology.concept_id
FROM concept psychology
JOIN concept_parent route ON route.concept_id = psychology.concept_id
JOIN concept parent ON parent.concept_id = route.parent_concept_id
WHERE psychology.preferred_label = 'Psychology'
  AND parent.preferred_label = 'Social Sciences';

CREATE TEMP TABLE mind_wrapper(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO mind_wrapper
SELECT child.concept_id
FROM concept_parent route
JOIN concept child ON child.concept_id = route.concept_id
WHERE route.parent_concept_id IN (SELECT concept_id FROM psychology_context)
  AND child.preferred_label = 'Mind, Brain & Behavior';

CREATE TEMP TABLE promoted_psychology(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO promoted_psychology VALUES
('unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02'),
('unified:8fd4ef30-39c5-527b-b72a-b788c51b1e6f'),
('unified:82acdd50-1fbb-5a2e-8360-cbedffd2329f'),
('bokheim:philosophy-lcc-full:153b3c0d-1ad9-51cb-939a-75e948fa084c'),
('bokheim:lcc-detail:651d361a-c770-5310-bf87-92659769dc9a'),
('bokheim:lcc-detail:39ff130e-bf33-5a0d-a6cc-242d9b5f112a'),
('bokheim:lcc-detail:f8b2d9a2-8577-5f96-8538-b62eca15f423'),
('unified:4ac88a2d-b697-5ba6-b96d-f1183f806df0'),
('unified:01a01f23-ace3-5538-b278-938880055953'),
('unified:3185f791-a707-5774-8ce4-97175ec93e21');

-- Consciousness is split from the combined historical Cognition caption. It
-- is navigation-only because the source selector covers the combined topic.
INSERT OR IGNORE INTO concept(concept_id, preferred_label)
VALUES ('bokheim:psychology:consciousness', 'Consciousness');
INSERT OR IGNORE INTO source_label(concept_id, label, ordinal)
VALUES ('bokheim:psychology:consciousness', 'Consciousness', 0);
INSERT OR IGNORE INTO promoted_psychology
VALUES ('bokheim:psychology:consciousness');

UPDATE concept SET preferred_label = 'Cognition'
WHERE concept_id = 'unified:b2a9e613-c68b-536d-b69a-f7bc3960ac02';
UPDATE concept SET preferred_label = 'Perception'
WHERE concept_id = 'unified:82acdd50-1fbb-5a2e-8360-cbedffd2329f';
UPDATE concept SET preferred_label = 'Intelligence'
WHERE concept_id = 'bokheim:lcc-detail:39ff130e-bf33-5a0d-a6cc-242d9b5f112a';
UPDATE concept SET preferred_label = 'Thought & Reasoning'
WHERE concept_id = 'bokheim:lcc-detail:f8b2d9a2-8577-5f96-8538-b62eca15f423';
UPDATE concept SET preferred_label = 'Cognitive Neuroscience'
WHERE concept_id = 'unified:4ac88a2d-b697-5ba6-b96d-f1183f806df0';

-- Promote all selected concepts directly under Psychology.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT promoted.concept_id,
       psychology.concept_id,
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = promoted.concept_id), 0),
       100
FROM promoted_psychology promoted
CROSS JOIN psychology_context psychology
WHERE NOT EXISTS (
    SELECT 1 FROM concept_parent existing
    WHERE existing.concept_id = promoted.concept_id
      AND existing.parent_concept_id = psychology.concept_id
);

CREATE TEMP TABLE mind_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT concept_id FROM mind_wrapper
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO mind_detail
SELECT concept_id FROM detail
WHERE concept_id NOT IN (SELECT concept_id FROM promoted_psychology);

-- Disconnect the obsolete wrapper tree, including old incoming routes to the
-- promoted concepts.
DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM mind_detail)
  AND concept_id IN (
      SELECT concept_id FROM mind_detail
      UNION SELECT concept_id FROM promoted_psychology
  );
DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM psychology_context)
  AND concept_id IN (SELECT concept_id FROM mind_wrapper);

-- Promoted subjects are leaves. Their former detailed children are already
-- represented in mind_detail and become unreachable from Psychology.
DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM promoted_psychology);

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM mind_detail)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id = concept.concept_id
         OR remaining.parent_concept_id = concept.concept_id
  );

DROP TABLE mind_detail;
DROP TABLE promoted_psychology;
DROP TABLE mind_wrapper;
DROP TABLE psychology_context;
COMMIT;
