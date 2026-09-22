-- Flatten the remaining Biography & Autobiography presentation wrappers.
-- Keep coded subject leaves, including Cultural, Ethnic & Regional and its
-- children, while promoting the former Biography and Forms children to the
-- root.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

CREATE TEMP TABLE biography_root_children (concept_id INTEGER PRIMARY KEY);
INSERT INTO biography_root_children (concept_id) VALUES
  (3213),(3214),(3215),(3217),(3218),(3219),(3220),(3221),
  (3222),(3223),(3224),(3225),(3231),(3232),(3233),(3234),(3235),
  (3236),(3237),(3238),(3239),(3240),(3241),(3242),(3243),(3244),
  (3245),(3246),(3247),(3248),(3249),(3250),(3251),(3252),(3253),
  (3254),(3255),(3256),(3257),(3258),(3259);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM biography_root_children)
   OR concept_id IN (3216,5771)
   OR parent_concept_id IN (3216,5771,5765,5766,5767,5768,5769,5770);

INSERT INTO concept_parent (concept_id, parent_concept_id, ordinal)
SELECT concept_id, 3212,
       ROW_NUMBER() OVER (ORDER BY concept_id) - 1
FROM biography_root_children;

DELETE FROM lcc_selector WHERE concept_id IN (3216,5771);
DELETE FROM bisac_selector WHERE concept_id IN (3216,5771);
DELETE FROM ddc_selector WHERE concept_id IN (3216,5771);
DELETE FROM source_label WHERE concept_id IN (3216,5771);
DELETE FROM concept WHERE concept_id IN (3216,5771);

DROP TABLE biography_root_children;
COMMIT;
