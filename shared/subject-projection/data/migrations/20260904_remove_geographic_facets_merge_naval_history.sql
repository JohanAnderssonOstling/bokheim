-- Remove unwanted geographic facet branches throughout the curated taxonomy,
-- and consolidate Naval history into Military history at the same level.
-- Full source structure remains in master.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE geographic_facet_detail(concept_id TEXT PRIMARY KEY) WITHOUT ROWID;
WITH RECURSIVE detail(concept_id) AS (
 SELECT concept_id FROM concept
 WHERE lower(preferred_label) IN (
   'descriptive geography',
   'historical geography',
   'monumental and picturesque',
   'place names (general)',
   'special aspects'
 )
 UNION
 SELECT child.concept_id
 FROM detail parent
 JOIN concept_parent child ON child.parent_concept_id=parent.concept_id
)
INSERT INTO geographic_facet_detail SELECT concept_id FROM detail;

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM geographic_facet_detail)
   OR parent_concept_id IN (SELECT concept_id FROM geographic_facet_detail);
DELETE FROM concept WHERE concept_id IN (SELECT concept_id FROM geographic_facet_detail);

CREATE TEMP TABLE naval_merge(source_id TEXT PRIMARY KEY,target_id TEXT NOT NULL) WITHOUT ROWID;
INSERT INTO naval_merge
SELECT naval.concept_id,military.concept_id
FROM concept naval
JOIN concept_parent naval_edge ON naval_edge.concept_id=naval.concept_id
JOIN concept_parent military_edge ON military_edge.parent_concept_id=naval_edge.parent_concept_id
JOIN concept military ON military.concept_id=military_edge.concept_id
WHERE lower(naval.preferred_label)='naval history'
  AND lower(military.preferred_label)='military history';

INSERT INTO source_label(concept_id,label,ordinal)
SELECT merge.target_id,source.label,
       COALESCE((SELECT max(ordinal) FROM source_label
                 WHERE concept_id=merge.target_id),-1)
       + row_number() OVER (PARTITION BY merge.target_id ORDER BY source.ordinal,source.label)
FROM naval_merge merge
JOIN source_label source ON source.concept_id=merge.source_id
WHERE NOT EXISTS (
 SELECT 1 FROM source_label existing
 WHERE existing.concept_id=merge.target_id AND existing.label=source.label
);

INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT merge.target_id,source.system_id,source.selector,
       COALESCE((SELECT max(ordinal) FROM source_selector
                 WHERE concept_id=merge.target_id AND system_id=source.system_id),-1)
       + row_number() OVER (
           PARTITION BY merge.target_id,source.system_id
           ORDER BY source.ordinal,source.selector
         )
FROM naval_merge merge
JOIN source_selector source ON source.concept_id=merge.source_id
WHERE NOT EXISTS (
 SELECT 1 FROM source_selector existing
 WHERE existing.concept_id=merge.target_id
   AND existing.system_id=source.system_id
   AND existing.selector=source.selector
);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT source_id FROM naval_merge)
   OR parent_concept_id IN (SELECT source_id FROM naval_merge);
DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM naval_merge);

-- The few naval categories without a military sibling already represent the
-- same broader concern; retain their selectors and normalize their label.
UPDATE concept SET preferred_label='Military history'
WHERE lower(preferred_label)='naval history';

DROP TABLE naval_merge;
DROP TABLE geographic_facet_detail;
COMMIT;
