-- Extract useful Sociology topics into existing branches; keep HM851 and new subjects as leaves.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO source_selector VALUES('unified:2f1ccb99-3ae5-5764-acfe-6f1139910d0d','lcc','HM846..HM846@6',0);
INSERT INTO concept VALUES('bokheim:sociology:information-society','Information Technology & Information Society');
INSERT INTO concept_parent VALUES('bokheim:sociology:information-society','bokheim:sociology:technology-everyday',0,11);
INSERT INTO source_selector VALUES('bokheim:sociology:information-society','lcc','HM851..HM851@6',0);
INSERT INTO concept VALUES('bokheim:sociology:collective-behavior-movements','Collective Behavior & Social Movements');
INSERT INTO concept_parent VALUES('bokheim:sociology:collective-behavior-movements','bokheim:sociology:change-development',0,22);
INSERT INTO source_selector VALUES('bokheim:sociology:collective-behavior-movements','lcc','HM866..HM886@6',0);
INSERT INTO concept VALUES('bokheim:sociology:social-networks','Social Networks');
INSERT INTO concept_parent VALUES('bokheim:sociology:social-networks','bokheim:sociology:identity-community',0,34);
INSERT INTO source_selector VALUES('bokheim:sociology:social-networks','lcc','HM741..HM743@6',0);
INSERT INTO concept VALUES('bokheim:sociology:group-social-identity','Group & Social Identity');
INSERT INTO concept_parent VALUES('bokheim:sociology:group-social-identity','bokheim:sociology:identity-community',0,35);
INSERT INTO source_selector VALUES('bokheim:sociology:group-social-identity','lcc','HM753..HM755@6',0);
INSERT INTO source_selector VALUES('unified:70ccf5dd-18b9-5a3c-b6d7-a09e38bec32c','lcc','HM671..HM671@6',0);
COMMIT;
