PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE year_merge(old_id INTEGER PRIMARY KEY,new_id INTEGER NOT NULL);
-- Resolve nested numeric periods directly to their surviving parent.
INSERT INTO year_merge VALUES(3412,3404),(3413,3405),
 (50990,3405),(50991,3405),(50992,3405),(51008,3405),(51013,3405),
 (51337,3409),(51338,3409);
WITH names AS(
 SELECT new_id,label FROM year_merge JOIN source_label ON concept_id=old_id
 UNION SELECT new_id,preferred_label FROM year_merge JOIN concept ON concept_id=old_id
), fresh AS(
 SELECT * FROM names n WHERE NOT EXISTS(SELECT 1 FROM source_label s WHERE s.concept_id=n.new_id AND s.label=n.label)
)
INSERT INTO source_label SELECT new_id,label,
 coalesce((SELECT max(ordinal)+1 FROM source_label WHERE concept_id=new_id),0)
 + row_number() OVER(PARTITION BY new_id ORDER BY label)-1 FROM fresh;
INSERT OR IGNORE INTO lcc_selector SELECT new_id,selector FROM lcc_selector JOIN year_merge ON concept_id=old_id;
INSERT OR IGNORE INTO bisac_selector SELECT new_id,selector FROM bisac_selector JOIN year_merge ON concept_id=old_id;
UPDATE lcc_range SET concept_id=(SELECT new_id FROM year_merge WHERE old_id=concept_id) WHERE concept_id IN(SELECT old_id FROM year_merge);
UPDATE ddc_selector SET concept_id=(SELECT new_id FROM year_merge WHERE old_id=concept_id) WHERE concept_id IN(SELECT old_id FROM year_merge);
-- Rehome children, deduplicate parents, and preserve their previous order.
CREATE TEMP TABLE changed_children AS
 SELECT DISTINCT concept_id FROM concept_parent WHERE parent_concept_id IN(SELECT old_id FROM year_merge) AND concept_id NOT IN(SELECT old_id FROM year_merge);
CREATE TEMP TABLE replacement_parents AS
 SELECT p.concept_id,coalesce(m.new_id,p.parent_concept_id) parent_id,min(p.ordinal) old_ordinal
 FROM concept_parent p LEFT JOIN year_merge m ON m.old_id=p.parent_concept_id
 WHERE p.concept_id IN(SELECT concept_id FROM changed_children)
 GROUP BY p.concept_id,coalesce(m.new_id,p.parent_concept_id);
DELETE FROM concept_parent WHERE concept_id IN(SELECT concept_id FROM changed_children) OR concept_id IN(SELECT old_id FROM year_merge);
INSERT INTO concept_parent SELECT concept_id,parent_id,row_number() OVER(PARTITION BY concept_id ORDER BY old_ordinal,parent_id)-1 FROM replacement_parents;
DELETE FROM concept WHERE concept_id IN(SELECT old_id FROM year_merge);
-- Child range fragments now wholly covered by the same parent's fallback.
DELETE FROM lcc_range WHERE concept_id=3405 AND start_code IN('E838','E841','E852','E876','E751','E784');
DELETE FROM lcc_range WHERE concept_id=3409 AND start_code IN('E191','E195');
DROP TABLE replacement_parents;
DROP TABLE changed_children;
DROP TABLE year_merge;
COMMIT;
