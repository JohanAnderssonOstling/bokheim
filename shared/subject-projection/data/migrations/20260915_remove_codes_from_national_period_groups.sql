-- Period grouping nodes must not carry classification codes. Transfer their
-- coverage to leaf descendants within the same national By Period tree.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE period_descendant(by_period_id INTEGER NOT NULL, concept_id INTEGER NOT NULL, PRIMARY KEY (by_period_id,concept_id)) WITHOUT ROWID;
WITH RECURSIVE history(concept_id) AS (
    SELECT concept_id FROM concept WHERE lower(preferred_label)='history'
    UNION SELECT child.concept_id FROM history parent JOIN concept_parent edge ON edge.parent_concept_id=parent.concept_id JOIN concept child ON child.concept_id=edge.concept_id
), by_period(concept_id) AS (
    SELECT concept.concept_id FROM history JOIN concept ON concept.concept_id=history.concept_id WHERE lower(concept.preferred_label)='by period'
), descendants(by_period_id,concept_id) AS (
    SELECT concept_id,concept_id FROM by_period
    UNION SELECT descendants.by_period_id,child.concept_id FROM descendants JOIN concept_parent edge ON edge.parent_concept_id=descendants.concept_id JOIN concept child ON child.concept_id=edge.concept_id
)
INSERT INTO period_descendant SELECT by_period_id,concept_id FROM descendants;

CREATE TEMP TABLE period_nonleaf(by_period_id INTEGER NOT NULL, concept_id INTEGER NOT NULL, PRIMARY KEY (by_period_id,concept_id)) WITHOUT ROWID;
INSERT INTO period_nonleaf
SELECT parent.by_period_id,parent.concept_id FROM period_descendant parent
WHERE EXISTS (SELECT 1 FROM concept_parent edge JOIN period_descendant child ON child.by_period_id=parent.by_period_id AND child.concept_id=edge.concept_id WHERE edge.parent_concept_id=parent.concept_id);

CREATE TEMP TABLE period_leaf(by_period_id INTEGER NOT NULL, concept_id INTEGER NOT NULL, PRIMARY KEY (by_period_id,concept_id)) WITHOUT ROWID;
INSERT INTO period_leaf
SELECT descendant.by_period_id,descendant.concept_id FROM period_descendant descendant
WHERE NOT EXISTS (SELECT 1 FROM concept_parent edge JOIN period_descendant child ON child.by_period_id=descendant.by_period_id AND child.concept_id=edge.concept_id WHERE edge.parent_concept_id=descendant.concept_id);

CREATE TEMP TABLE period_code_assignment(by_period_id INTEGER NOT NULL, source_id INTEGER NOT NULL, leaf_id INTEGER NOT NULL, PRIMARY KEY (by_period_id,source_id,leaf_id)) WITHOUT ROWID;
WITH RECURSIVE walk(by_period_id,source_id,current_id) AS (
    SELECT by_period_id,concept_id,concept_id FROM period_nonleaf
    UNION SELECT walk.by_period_id,walk.source_id,edge.concept_id FROM walk JOIN concept_parent edge ON edge.parent_concept_id=walk.current_id JOIN period_descendant child ON child.by_period_id=walk.by_period_id AND child.concept_id=edge.concept_id
)
INSERT INTO period_code_assignment
SELECT walk.by_period_id,walk.source_id,walk.current_id FROM walk JOIN period_leaf leaf ON leaf.by_period_id=walk.by_period_id AND leaf.concept_id=walk.current_id;

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT range.start_code,range.end_code,assignment.leaf_id FROM lcc_range range JOIN period_code_assignment assignment ON assignment.source_id=range.concept_id;
DELETE FROM lcc_range WHERE concept_id IN (SELECT DISTINCT concept_id FROM period_nonleaf);
INSERT OR IGNORE INTO bisac_selector(concept_id,selector)
SELECT assignment.leaf_id,selector.selector FROM bisac_selector selector JOIN period_code_assignment assignment ON assignment.source_id=selector.concept_id;
DELETE FROM bisac_selector WHERE concept_id IN (SELECT DISTINCT concept_id FROM period_nonleaf);

DROP TABLE period_code_assignment;
DROP TABLE period_leaf;
DROP TABLE period_nonleaf;
DROP TABLE period_descendant;
COMMIT;
