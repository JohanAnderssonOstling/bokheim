PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Unify the two BISAC rights concepts with the general/compare LCC rights
-- concept. Jurisdiction-specific constitutional rights remain in country law.
CREATE TEMP TABLE rights_merge(source_id TEXT PRIMARY KEY) WITHOUT ROWID;
INSERT INTO rights_merge VALUES
('unified:001bba66-64da-58dd-8aa9-a68909f1c07d'),
('bokheim:law:comparative:rights');

WITH candidates AS (
 SELECT DISTINCT selector.system_id,selector.selector
 FROM rights_merge merge
 JOIN source_selector selector ON selector.concept_id=merge.source_id
 WHERE NOT EXISTS (
  SELECT 1 FROM source_selector existing
  WHERE existing.concept_id='unified:d292d3d5-5619-561d-b79e-6ccec11024ba'
    AND existing.system_id=selector.system_id
    AND existing.selector=selector.selector
 )
), numbered AS (
 SELECT candidates.*,
        row_number() OVER (PARTITION BY system_id ORDER BY selector)-1 AS offset
 FROM candidates
)
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT 'unified:d292d3d5-5619-561d-b79e-6ccec11024ba',system_id,selector,
       COALESCE((SELECT max(existing.ordinal)+1 FROM source_selector existing
                 WHERE existing.concept_id='unified:d292d3d5-5619-561d-b79e-6ccec11024ba'
                   AND existing.system_id=numbered.system_id),0)+offset
FROM numbered;

DELETE FROM concept WHERE concept_id IN (SELECT source_id FROM rights_merge);
UPDATE concept SET preferred_label='Human & Civil Rights'
WHERE concept_id='unified:d292d3d5-5619-561d-b79e-6ccec11024ba';
DROP TABLE rights_merge;

-- JF799-JF1177 is explicitly Political rights / Political participation in
-- general and comparative government. Move only those scoped selectors out of
-- the broad public-administration concept; retain its wider JF selectors.
UPDATE source_selector
SET concept_id='unified:7ecd6334-bc84-5f4e-87a9-b82c698a5d84',
    ordinal=CASE selector
      WHEN 'JF799..JF1177@3' THEN 0
      WHEN 'JF799..JF1197.2@4' THEN 1
    END
WHERE concept_id='unified:d559fba0-8aae-58dd-9df0-7e459aef7cb2'
  AND system_id='lcc'
  AND selector IN ('JF799..JF1177@3','JF799..JF1197.2@4');

UPDATE concept SET preferred_label='Civics, Citizenship & Political Participation'
WHERE concept_id='unified:7ecd6334-bc84-5f4e-87a9-b82c698a5d84';

COMMIT;
