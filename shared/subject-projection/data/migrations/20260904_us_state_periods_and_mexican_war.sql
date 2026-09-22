-- Reduce every curated United States state to direct historical periods, and
-- consolidate the duplicated Mexican War hierarchy. Full detail remains in
-- the master taxonomy.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE us_state_name(name TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO us_state_name VALUES
 ('Alabama'),('Alaska'),('Arizona'),('Arkansas'),('California'),('Colorado'),
 ('Connecticut'),('Delaware'),('Florida'),('Georgia'),('Hawaii'),('Idaho'),
 ('Illinois'),('Indiana'),('Iowa'),('Kansas'),('Kentucky'),('Louisiana'),
 ('Maine'),('Maryland'),('Massachusetts'),('Michigan'),('Minnesota'),
 ('Mississippi'),('Missouri'),('Montana'),('Nebraska'),('Nevada'),
 ('New Hampshire'),('New Jersey'),('New Mexico'),('New York'),
 ('North Carolina'),('North Dakota'),('Ohio'),('Oklahoma'),('Oregon'),
 ('Pennsylvania'),('Rhode Island'),('South Carolina'),('South Dakota'),
 ('Tennessee'),('Texas'),('Utah'),('Vermont'),('Virginia'),('Washington'),
 ('West Virginia'),('Wisconsin'),('Wyoming');

CREATE TEMP TABLE us_state(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE us_subtree(concept_id) AS (
    VALUES ('unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN us_subtree parent ON parent.concept_id=child.parent_concept_id
)
INSERT INTO us_state
SELECT item.concept_id
FROM us_subtree item
JOIN concept c ON c.concept_id=item.concept_id
JOIN us_state_name state_name ON state_name.name=c.preferred_label;

CREATE TEMP TABLE us_state_period(
    state_id TEXT NOT NULL,
    concept_id TEXT PRIMARY KEY,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;

-- Retain the immediate period categories, but not topical detail nested below
-- those periods.
INSERT INTO us_state_period
SELECT state.concept_id, period.concept_id, period.display_order
FROM us_state state
JOIN concept_parent wrapper ON wrapper.parent_concept_id=state.concept_id
JOIN concept wrapper_item ON wrapper_item.concept_id=wrapper.concept_id
JOIN concept_parent period ON period.parent_concept_id=wrapper.concept_id
WHERE lower(wrapper_item.preferred_label)='by period';

-- Texas has a genuine chronological split nested below an open-ended range.
INSERT OR IGNORE INTO us_state_period
SELECT state.concept_id, period.concept_id, 10000 + period.display_order
FROM us_state state
JOIN concept state_item ON state_item.concept_id=state.concept_id
JOIN concept_parent first_period ON first_period.parent_concept_id=state.concept_id
JOIN concept first_wrapper ON first_wrapper.concept_id=first_period.concept_id
JOIN concept_parent period_group ON period_group.parent_concept_id=first_wrapper.concept_id
JOIN concept group_item ON group_item.concept_id=period_group.concept_id
JOIN concept_parent period ON period.parent_concept_id=period_group.concept_id
JOIN concept period_item ON period_item.concept_id=period.concept_id
WHERE state_item.preferred_label='Texas'
  AND lower(first_wrapper.preferred_label)='by period'
  AND group_item.preferred_label='1846-'
  AND period_item.preferred_label='1951-';

CREATE TEMP TABLE us_state_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE descendants(concept_id) AS (
    SELECT child.concept_id
    FROM us_state state
    JOIN concept_parent child ON child.parent_concept_id=state.concept_id
    UNION
    SELECT child.concept_id
    FROM descendants parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO us_state_detail
SELECT concept_id FROM descendants
WHERE concept_id NOT IN (SELECT concept_id FROM us_state_period);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_state_period)
  AND parent_concept_id IN (
      SELECT concept_id FROM us_state_detail
      UNION SELECT concept_id FROM us_state
  );

-- A retained period may itself have been nested below another retained period
-- (Texas 1951-); remove that old edge before attaching both to the state.
DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_state_period)
  AND parent_concept_id IN (SELECT concept_id FROM us_state_period);

INSERT INTO concept_parent(concept_id, parent_concept_id, ordinal, display_order)
SELECT period.concept_id,
       period.state_id,
       COALESCE((SELECT max(ordinal) + 1 FROM concept_parent WHERE concept_id=period.concept_id), 0),
       period.display_order
FROM us_state_period period;

-- Close the former open-ended Texas range at the next retained period.
UPDATE concept SET preferred_label='1846-1950'
WHERE concept_id IN (
    SELECT period.concept_id
    FROM us_state_period period
    JOIN concept item ON item.concept_id=period.concept_id
    JOIN concept state_item ON state_item.concept_id=period.state_id
    WHERE state_item.preferred_label='Texas' AND item.preferred_label='1846-'
);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM us_state_detail)
   OR parent_concept_id IN (SELECT concept_id FROM us_state_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM us_state_detail);

-- Merge the detailed duplicate Mexican War node into the canonical historical
-- era, then leave the canonical entry as a leaf.
INSERT INTO source_selector(concept_id, system_id, selector, ordinal)
SELECT 'unified:44463786-4323-5f18-9a1a-c89e2d8be8e8', source.system_id, source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id='unified:44463786-4323-5f18-9a1a-c89e2d8be8e8'
                   AND system_id=source.system_id), -1)
       + row_number() OVER (PARTITION BY source.system_id ORDER BY source.ordinal)
FROM source_selector source
WHERE source.concept_id='bokheim:history-lcc:d25070e9-704b-578c-98c2-1462aeff6d7c'
  AND NOT EXISTS (
      SELECT 1 FROM source_selector existing
      WHERE existing.concept_id='unified:44463786-4323-5f18-9a1a-c89e2d8be8e8'
        AND existing.system_id=source.system_id
        AND existing.selector=source.selector
  );

CREATE TEMP TABLE mexican_war_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
    SELECT child.concept_id FROM concept_parent child
    WHERE child.parent_concept_id IN (
        'unified:44463786-4323-5f18-9a1a-c89e2d8be8e8',
        'bokheim:history-lcc:d25070e9-704b-578c-98c2-1462aeff6d7c'
    )
    UNION
    SELECT child.concept_id
    FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO mexican_war_detail SELECT concept_id FROM detail;
INSERT OR IGNORE INTO mexican_war_detail VALUES
    ('bokheim:history-lcc:d25070e9-704b-578c-98c2-1462aeff6d7c');

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM mexican_war_detail)
   OR parent_concept_id IN (SELECT concept_id FROM mexican_war_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM mexican_war_detail);

-- Presidential administrations remain useful periods, but their nested event
-- and policy detail is too fine-grained for the curated hierarchy.
CREATE TEMP TABLE presidential_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE
us_subtree(concept_id) AS (
    VALUES ('unified:1fd4042d-5f0d-5835-ac17-9bc708487a6a')
    UNION
    SELECT child.concept_id
    FROM concept_parent child
    JOIN us_subtree parent ON parent.concept_id=child.parent_concept_id
),
administration(concept_id) AS (
    SELECT item.concept_id
    FROM us_subtree item
    JOIN concept c ON c.concept_id=item.concept_id
    WHERE lower(c.preferred_label) LIKE '%''s administration%'
),
detail(concept_id) AS (
    SELECT child.concept_id
    FROM administration parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
    UNION
    SELECT child.concept_id
    FROM detail parent
    JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO presidential_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM presidential_detail)
   OR parent_concept_id IN (SELECT concept_id FROM presidential_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM presidential_detail);

DROP TABLE presidential_detail;
DROP TABLE mexican_war_detail;
DROP TABLE us_state_detail;
DROP TABLE us_state_period;
DROP TABLE us_state;
DROP TABLE us_state_name;
COMMIT;
