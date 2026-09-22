-- Curated U.S. periods: retain eras, not individual administrations.
-- Reference: LOC LCC E-F outline (2022); BISG History headings.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE administration_merge(old_id INTEGER PRIMARY KEY,new_id INTEGER NOT NULL);
INSERT INTO administration_merge VALUES
 (51012,3405),(51017,3405),(51018,3405),(51019,3405),
 (51082,3406),(51083,3406),(51084,3406),(51085,3406),
 (51145,3412),(51146,3412),(51147,3412),(51148,3412),
 (51149,3412),(51150,3412),(51151,3412),(51152,3412),(51153,3412);
-- Preserve searchable names and all selectors before deleting the subjects.
WITH names AS (
 SELECT new_id,label FROM administration_merge JOIN source_label ON concept_id=old_id
 UNION SELECT new_id,preferred_label FROM administration_merge JOIN concept ON concept_id=old_id
), fresh AS (
 SELECT * FROM names n WHERE NOT EXISTS(SELECT 1 FROM source_label s WHERE s.concept_id=n.new_id AND s.label=n.label)
)
INSERT INTO source_label SELECT new_id,label,
 coalesce((SELECT max(ordinal)+1 FROM source_label WHERE concept_id=new_id),0)
 + row_number() OVER(PARTITION BY new_id ORDER BY label)-1 FROM fresh;
INSERT OR IGNORE INTO lcc_selector SELECT new_id,selector FROM lcc_selector JOIN administration_merge ON concept_id=old_id;
INSERT OR IGNORE INTO bisac_selector SELECT new_id,selector FROM bisac_selector JOIN administration_merge ON concept_id=old_id;
UPDATE lcc_range SET concept_id=(SELECT new_id FROM administration_merge WHERE old_id=concept_id) WHERE concept_id IN(SELECT old_id FROM administration_merge);
UPDATE ddc_selector SET concept_id=(SELECT new_id FROM administration_merge WHERE old_id=concept_id) WHERE concept_id IN(SELECT old_id FROM administration_merge);
-- These administration subjects have no children (checked before application).
DELETE FROM concept WHERE concept_id IN(SELECT old_id FROM administration_merge);

-- Restore the complete constitutional range to its existing period.
DELETE FROM lcc_range WHERE concept_id=3403;
UPDATE lcc_range SET concept_id=3403 WHERE concept_id=3402 AND start_code='E310' AND end_code='E337';
-- E337.8 belongs to the early period, not the century's general heading.
UPDATE lcc_range SET end_code='E337.799' WHERE concept_id=3404 AND start_code='E337.5';
DELETE FROM lcc_range WHERE concept_id IN(3411,3415);
INSERT INTO lcc_range VALUES ('E337.8','E400.999',3411),('E415.6','E440.5',3415);
UPDATE lcc_range SET end_code='E889.999' WHERE concept_id=3413 AND start_code='E838';
-- Late-eighteenth-century biography is not specifically Confederation history.
UPDATE lcc_range SET start_code='E303' WHERE concept_id=51100 AND start_code='E302.5';
INSERT INTO lcc_range VALUES ('E302.5','E302.699',3417);
UPDATE lcc_selector SET concept_id=51100 WHERE concept_id=3402 AND selector='E303-E309';
UPDATE lcc_selector SET concept_id=3404 WHERE concept_id=3402 AND selector='E337.5';

-- Keep the full LCC era as fallback; specific topical children still win.
DELETE FROM lcc_range WHERE concept_id=3412;
INSERT INTO lcc_range VALUES ('E660','E738.999',3412);
-- The equivalent merged exact codes are now represented by that range.
DELETE FROM lcc_selector WHERE concept_id=3412 AND selector IN('E670','E675','E680','E685','E695','E700','E705','E710','E738');

-- Remove redundant direct routes when a period already has an era parent.
DELETE FROM concept_parent WHERE parent_concept_id=3407 AND concept_id IN(3403,3411,3414,3415);
UPDATE concept_parent SET parent_concept_id=3404 WHERE concept_id=3412 AND parent_concept_id=3407;
UPDATE concept_parent SET parent_concept_id=3405 WHERE concept_id=3413 AND parent_concept_id=3407;
-- Keep topical late-century ranges descendants of the widened period fallback.
INSERT OR IGNORE INTO concept_parent
 SELECT c.concept_id,3413,coalesce((SELECT max(ordinal)+1 FROM concept_parent p WHERE p.concept_id=c.concept_id),0)
 FROM concept c WHERE c.concept_id IN(51005,51006,51007);
INSERT OR IGNORE INTO concept_parent
 SELECT 3693,3411,coalesce(max(ordinal)+1,0) FROM concept_parent WHERE concept_id=3693;

UPDATE concept SET preferred_label='Early 19th Century, 1801/1809–1845' WHERE concept_id=3411;
UPDATE concept SET preferred_label='Revolution to Civil War, 1775/1783–1861' WHERE concept_id=3417;
UPDATE concept SET preferred_label='1901–1913' WHERE concept_id=51008;
UPDATE concept SET preferred_label='1919–1933' WHERE concept_id=51013;
UPDATE concept SET preferred_label='Political History, 1961–2000' WHERE concept_id=51005;
UPDATE concept SET preferred_label='Foreign Relations, 1961–2000' WHERE concept_id=51006;
UPDATE concept SET preferred_label='Biography, 1961–2000' WHERE concept_id=51007;
-- LCC deliberately uses nominal century boundaries (McKinley/Clinton spill
-- into 1901/2001). Do not invent disjoint periods or narrow BISAC to LCC dates.
DROP TABLE administration_merge;
COMMIT;
