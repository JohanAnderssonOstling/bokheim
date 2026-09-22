-- Limit national History period branches to two levels below By Period.
-- Coverage from deeper descendants is retained on the second-level period
-- (for example, Italian History / Ancient Era / Roman Empire).

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_descendant (
    by_period_id INTEGER NOT NULL,
    concept_id INTEGER NOT NULL,
    relative_depth INTEGER NOT NULL,
    second_level_id INTEGER,
    PRIMARY KEY (by_period_id, concept_id, second_level_id)
) WITHOUT ROWID;

WITH RECURSIVE
history(concept_id) AS (
    SELECT concept_id FROM concept WHERE lower(preferred_label)='history'
    UNION
    SELECT child.concept_id
    FROM history parent
    JOIN concept_parent edge ON edge.parent_concept_id=parent.concept_id
    JOIN concept child ON child.concept_id=edge.concept_id
),
by_period(concept_id) AS (
    SELECT concept.concept_id
    FROM history
    JOIN concept ON concept.concept_id=history.concept_id
    WHERE lower(concept.preferred_label)='by period'
),
descendants(by_period_id,concept_id,relative_depth,second_level_id) AS (
    SELECT concept_id,concept_id,0,NULL FROM by_period
    UNION ALL
    SELECT descendants.by_period_id,
           child.concept_id,
           descendants.relative_depth+1,
           CASE WHEN descendants.relative_depth+1=2
                THEN child.concept_id ELSE descendants.second_level_id END
    FROM descendants
    JOIN concept_parent edge ON edge.parent_concept_id=descendants.concept_id
    JOIN concept child ON child.concept_id=edge.concept_id
)
INSERT OR IGNORE INTO period_descendant
SELECT by_period_id,concept_id,relative_depth,second_level_id
FROM descendants;

CREATE TEMP TABLE deep_concept(concept_id INTEGER PRIMARY KEY) WITHOUT ROWID;
INSERT INTO deep_concept
SELECT DISTINCT concept_id FROM period_descendant WHERE relative_depth>=3;

CREATE TEMP TABLE deep_assignment(
    concept_id INTEGER NOT NULL,
    second_level_id INTEGER NOT NULL,
    PRIMARY KEY (concept_id,second_level_id)
) WITHOUT ROWID;
INSERT INTO deep_assignment
SELECT DISTINCT concept_id,second_level_id
FROM period_descendant
WHERE relative_depth>=3 AND second_level_id IS NOT NULL;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT range.start_code,range.end_code,assignment.second_level_id
FROM lcc_range range
JOIN deep_assignment assignment ON assignment.concept_id=range.concept_id;
DELETE FROM lcc_range WHERE concept_id IN (SELECT concept_id FROM deep_concept);

INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT assignment.second_level_id,selector.selector
FROM bisac_selector selector
JOIN deep_assignment assignment ON assignment.concept_id=selector.concept_id;
DELETE FROM bisac_selector WHERE concept_id IN (SELECT concept_id FROM deep_concept);

-- Remove only edges belonging to the affected By Period subgraphs, so a
-- concept shared with another taxonomy branch remains available there.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM deep_concept)
  AND parent_concept_id IN (
      SELECT concept_id FROM period_descendant
  );
DELETE FROM concept_parent
WHERE parent_concept_id IN (SELECT concept_id FROM deep_concept)
  AND concept_id IN (SELECT concept_id FROM period_descendant);

CREATE TEMP TABLE removed_deep_concept(concept_id INTEGER PRIMARY KEY) WITHOUT ROWID;
INSERT INTO removed_deep_concept
SELECT deep.concept_id
FROM deep_concept deep
WHERE NOT EXISTS (
    SELECT 1 FROM concept_parent edge
    WHERE edge.concept_id=deep.concept_id
);
DELETE FROM source_label WHERE concept_id IN (SELECT concept_id FROM removed_deep_concept);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM removed_deep_concept);

DROP TABLE removed_deep_concept;
DROP TABLE deep_assignment;
DROP TABLE deep_concept;
DROP TABLE period_descendant;

COMMIT;
