-- Consolidate Finance into a curated one-level subject list. Canonical Finance
-- children become leaves; detailed LCC structure remains in the master graph.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE finance_context(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO finance_context
SELECT finance.concept_id
FROM concept finance
JOIN concept_parent route ON route.concept_id = finance.concept_id
JOIN concept parent ON parent.concept_id = route.parent_concept_id
WHERE finance.preferred_label = 'Finance'
  AND parent.preferred_label = 'Economics & Business';

CREATE TEMP TABLE finance_merge(
    source_id TEXT PRIMARY KEY,
    target_id TEXT NOT NULL
) WITHOUT ROWID;

INSERT INTO finance_merge
SELECT source.concept_id, target.concept_id
FROM concept_parent source_route
JOIN concept source ON source.concept_id = source_route.concept_id
CROSS JOIN concept_parent target_route
JOIN concept target ON target.concept_id = target_route.concept_id
WHERE source_route.parent_concept_id IN (SELECT concept_id FROM finance_context)
  AND target_route.parent_concept_id = source_route.parent_concept_id
  AND (
      (source.preferred_label = 'Credit. Debt. Loans' AND target.preferred_label = 'Credit') OR
      (source.preferred_label = 'Finance management. Business finance.' AND target.preferred_label = 'Corporate Finance') OR
      (source.preferred_label = 'Foreign exchange. International finance.' AND target.preferred_label = 'Foreign Exchange') OR
      (source.preferred_label = 'Investment, capital formation, speculation' AND target.preferred_label = 'Investments & Securities') OR
      (source.preferred_label = 'Money' AND target.preferred_label = 'Money & Monetary Policy') OR
      (source.preferred_label = 'Life insurance' AND target.preferred_label = 'Insurance') OR
      (source.preferred_label = 'Trust services. Trust companies' AND target.preferred_label = 'Banking') OR
      (source.preferred_label = 'Liquidity' AND target.preferred_label = 'Money & Monetary Policy')
  );

-- Preserve source captions on the canonical concepts.
CREATE TEMP TABLE labels_to_move AS
SELECT merge.target_id AS concept_id, label.label,
       row_number() OVER (PARTITION BY merge.target_id ORDER BY label.label) - 1 AS offset
FROM finance_merge merge
JOIN source_label label ON label.concept_id = merge.source_id
WHERE NOT EXISTS (
    SELECT 1 FROM source_label existing
    WHERE existing.concept_id = merge.target_id AND existing.label = label.label
);

INSERT INTO source_label(concept_id, label, ordinal)
SELECT moved.concept_id, moved.label,
       COALESCE((SELECT max(existing.ordinal) + 1 FROM source_label existing
                 WHERE existing.concept_id = moved.concept_id), 0) + moved.offset
FROM labels_to_move moved;

-- Preserve classification selectors on the canonical concepts.
CREATE TEMP TABLE selectors_to_move AS
SELECT merge.target_id AS concept_id, selector.system_id, selector.selector,
       row_number() OVER (
           PARTITION BY merge.target_id, selector.system_id ORDER BY selector.selector
       ) - 1 AS offset
FROM finance_merge merge
JOIN source_selector selector ON selector.concept_id = merge.source_id
WHERE NOT EXISTS (
    SELECT 1 FROM source_selector existing
    WHERE existing.concept_id = merge.target_id
      AND existing.system_id = selector.system_id
      AND existing.selector = selector.selector
);

INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT moved.concept_id, moved.system_id, moved.selector,
       COALESCE((SELECT max(existing.ordinal) + 1 FROM source_selector existing
                 WHERE existing.concept_id = moved.concept_id
                   AND existing.system_id = moved.system_id), 0) + moved.offset
FROM selectors_to_move moved;

-- Remove merged headings and Lotteries from the Finance route.
CREATE TEMP TABLE removed_finance_child(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO removed_finance_child SELECT source_id FROM finance_merge;
INSERT OR IGNORE INTO removed_finance_child
SELECT child.concept_id
FROM concept_parent route
JOIN concept child ON child.concept_id = route.concept_id
WHERE route.parent_concept_id IN (SELECT concept_id FROM finance_context)
  AND child.preferred_label = 'Lotteries';

DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM finance_context)
  AND concept_id IN (SELECT concept_id FROM removed_finance_child);

-- Rename the surviving broad credit concept after consolidation.
UPDATE concept
SET preferred_label = 'Credit & Loans'
WHERE preferred_label = 'Credit'
  AND concept_id IN (
      SELECT child.concept_id FROM concept_parent child
      WHERE child.parent_concept_id IN (SELECT concept_id FROM finance_context)
  );

CREATE TEMP TABLE retained_finance_child(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO retained_finance_child
SELECT child.concept_id
FROM concept_parent child
WHERE child.parent_concept_id IN (SELECT concept_id FROM finance_context);

CREATE TEMP TABLE finance_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM retained_finance_child)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN detail parent ON parent.concept_id = child.parent_concept_id
), removed_detail(concept_id) AS (
    SELECT child.concept_id
    FROM concept_parent child
    WHERE child.parent_concept_id IN (SELECT concept_id FROM removed_finance_child)
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN removed_detail parent ON parent.concept_id = child.parent_concept_id
)
INSERT INTO finance_detail
SELECT concept_id FROM detail
UNION
SELECT concept_id FROM removed_detail
UNION
SELECT concept_id FROM removed_finance_child;

-- Detach the pruned Finance-only structure while preserving unrelated routes.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM finance_detail)
  AND parent_concept_id IN (
      SELECT concept_id FROM retained_finance_child
      UNION SELECT concept_id FROM removed_finance_child
      UNION SELECT concept_id FROM finance_detail
  );

DELETE FROM concept
WHERE concept_id IN (SELECT concept_id FROM finance_detail)
  AND NOT EXISTS (
      SELECT 1 FROM concept_parent remaining
      WHERE remaining.concept_id = concept.concept_id
         OR remaining.parent_concept_id = concept.concept_id
  );

DROP TABLE finance_detail;
DROP TABLE retained_finance_child;
DROP TABLE removed_finance_child;
DROP TABLE selectors_to_move;
DROP TABLE labels_to_move;
DROP TABLE finance_merge;
DROP TABLE finance_context;
COMMIT;
