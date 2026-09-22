-- Three direct author-period children under Canadian; retain general PR and alternative PS codes on parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:canadian-literature:through-1900','Authors through 1900');
INSERT INTO concept_parent VALUES('bokheim:canadian-literature:through-1900','unified:41899964-8fb9-5d37-8981-6330055f2964',0,0);
INSERT INTO source_selector VALUES('bokheim:canadian-literature:through-1900','lcc','PR9199.2..PR9199.2@6',0);
INSERT INTO concept VALUES('bokheim:canadian-literature:20th-century','20th-century authors');
INSERT INTO concept_parent VALUES('bokheim:canadian-literature:20th-century','unified:41899964-8fb9-5d37-8981-6330055f2964',0,1);
INSERT INTO source_selector VALUES('bokheim:canadian-literature:20th-century','lcc','PR9199.3..PR9199.3@6',0);
INSERT INTO concept VALUES('bokheim:canadian-literature:21st-century','21st-century authors');
INSERT INTO concept_parent VALUES('bokheim:canadian-literature:21st-century','unified:41899964-8fb9-5d37-8981-6330055f2964',0,2);
INSERT INTO source_selector VALUES('bokheim:canadian-literature:21st-century','lcc','PR9199.4..PR9199.4@6',0);
COMMIT;
