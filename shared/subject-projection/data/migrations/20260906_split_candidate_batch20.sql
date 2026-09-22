PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 35. Separate equality from the broader social-stratification selector.
INSERT INTO concept VALUES ('bokheim:social-disparity:stratification','Social Stratification & Economic Disparity'),('bokheim:social-disparity:equality','Social Equality');
INSERT INTO concept_parent VALUES ('bokheim:social-disparity:stratification','unified:2ec424cc-7113-565a-a6b6-4e3209d6a4c6',0,100),('bokheim:social-disparity:equality','unified:2ec424cc-7113-565a-a6b6-4e3209d6a4c6',0,200);
DELETE FROM source_selector WHERE concept_id='unified:2ec424cc-7113-565a-a6b6-4e3209d6a4c6';
INSERT INTO source_selector VALUES ('bokheim:social-disparity:stratification','bisac','SOC050000',0),('bokheim:social-disparity:stratification','lcc','HM821..HM821.17@5',0),('bokheim:social-disparity:equality','lcc','HM146',0);

COMMIT;
