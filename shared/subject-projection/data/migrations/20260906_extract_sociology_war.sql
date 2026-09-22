-- Extract Sociology & War, including its former HM36.5 classification.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:sociology:war','Sociology & War');
INSERT INTO concept_parent VALUES('bokheim:sociology:war','unified:1dc4999e-4875-54c4-be21-e8b23a259be2',0,111);
INSERT INTO source_selector VALUES('bokheim:sociology:war','lcc','HM554..HM554@5',0);
INSERT INTO source_selector VALUES('bokheim:sociology:war','lcc','HM36.5..HM36.5@5',1);
COMMIT;
