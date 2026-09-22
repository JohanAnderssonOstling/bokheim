-- Fold anonymous English works into their period parents.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_selector (concept_id,selector)
SELECT CASE concept_id
         WHEN 8934 THEN 3912
         WHEN 7258 THEN 3915
         WHEN 7564 THEN 3913
         WHEN 8215 THEN 3914
       END, selector
FROM lcc_selector
WHERE concept_id IN (8934,7258,7564,8215);

INSERT OR IGNORE INTO concept_parent (concept_id,parent_concept_id,ordinal)
SELECT child.concept_id,
       CASE edge.parent_concept_id
         WHEN 8934 THEN 3912
         WHEN 7258 THEN 3915
         WHEN 7564 THEN 3913
         WHEN 8215 THEN 3914
       END,
       edge.ordinal
FROM concept_parent edge
JOIN concept_parent child_edge ON child_edge.parent_concept_id=edge.concept_id
JOIN concept child ON child.concept_id=child_edge.concept_id
JOIN (SELECT concept_id,parent_concept_id,ordinal FROM concept_parent) child_edge2
  ON child_edge2.concept_id=child.concept_id
WHERE edge.concept_id IN (8934,7258,7564,8215);

DELETE FROM concept_parent WHERE concept_id IN (8934,7258,7564,8215)
   OR parent_concept_id IN (8934,7258,7564,8215);
DELETE FROM lcc_selector WHERE concept_id IN (8934,7258,7564,8215);
DELETE FROM concept WHERE concept_id IN (8934,7258,7564,8215);

COMMIT;
