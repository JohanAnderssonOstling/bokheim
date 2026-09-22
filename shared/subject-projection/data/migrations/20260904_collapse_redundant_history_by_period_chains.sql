-- Collapse only History -> By Period chains where History has no other child.
-- Meaningful History groupings elsewhere remain unchanged.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE redundant_history_period(
    history_id TEXT PRIMARY KEY,
    wrapper_id TEXT NOT NULL UNIQUE
) WITHOUT ROWID;

INSERT INTO redundant_history_period(history_id, wrapper_id)
SELECT edge.parent_concept_id, edge.concept_id
FROM concept_parent edge
JOIN concept history ON history.concept_id = edge.parent_concept_id
JOIN concept wrapper ON wrapper.concept_id = edge.concept_id
WHERE lower(trim(history.preferred_label)) = 'history'
  AND lower(trim(wrapper.preferred_label)) = 'by period'
  AND (SELECT count(*) FROM concept_parent child WHERE child.parent_concept_id = history.concept_id) = 1;

-- Preserve selectors carried by a terminal By Period node.
INSERT OR IGNORE INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT pair.history_id,
       selector.system_id,
       selector.selector,
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM source_selector existing
                 WHERE existing.concept_id = pair.history_id
                   AND existing.system_id = selector.system_id), 0)
FROM redundant_history_period pair
JOIN source_selector selector ON selector.concept_id = pair.wrapper_id;

-- Promote period entries directly under History.
INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT child.concept_id,
       pair.history_id,
       COALESCE((SELECT max(existing.ordinal) + 1
                 FROM concept_parent existing
                 WHERE existing.concept_id = child.concept_id), 0),
       child.display_order
FROM redundant_history_period pair
JOIN concept_parent child ON child.parent_concept_id = pair.wrapper_id
WHERE NOT EXISTS (
    SELECT 1 FROM concept_parent existing
    WHERE existing.concept_id = child.concept_id
      AND existing.parent_concept_id = pair.history_id
);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT wrapper_id FROM redundant_history_period)
   OR parent_concept_id IN (SELECT wrapper_id FROM redundant_history_period);

DELETE FROM concept
WHERE concept_id IN (SELECT wrapper_id FROM redundant_history_period);

DROP TABLE redundant_history_period;
COMMIT;
