-- Seven large countries directly under painting history by region or country.
-- Retain smaller countries and regional works on the parent.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT INTO concept VALUES('bokheim:painting-history:italy','Italy');
INSERT INTO concept_parent VALUES('bokheim:painting-history:italy','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,0);
INSERT INTO source_selector VALUES('bokheim:painting-history:italy','lcc','ND611..ND623.3@5',0);
INSERT INTO concept VALUES('bokheim:painting-history:france','France');
INSERT INTO concept_parent VALUES('bokheim:painting-history:france','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,1);
INSERT INTO source_selector VALUES('bokheim:painting-history:france','lcc','ND541..ND553.3@5',0);
INSERT INTO concept VALUES('bokheim:painting-history:united-states','United States');
INSERT INTO concept_parent VALUES('bokheim:painting-history:united-states','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,2);
INSERT INTO source_selector VALUES('bokheim:painting-history:united-states','lcc','ND205..ND238@5',0);
INSERT INTO concept VALUES('bokheim:painting-history:china','China');
INSERT INTO concept_parent VALUES('bokheim:painting-history:china','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,3);
INSERT INTO source_selector VALUES('bokheim:painting-history:china','lcc','ND1040..ND1049.6@5',0);
INSERT INTO concept VALUES('bokheim:painting-history:germany','Germany');
INSERT INTO concept_parent VALUES('bokheim:painting-history:germany','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,4);
INSERT INTO source_selector VALUES('bokheim:painting-history:germany','lcc','ND561..ND588.3@5',0);
INSERT INTO concept VALUES('bokheim:painting-history:great-britain','Great Britain & England');
INSERT INTO concept_parent VALUES('bokheim:painting-history:great-britain','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,5);
INSERT INTO source_selector VALUES('bokheim:painting-history:great-britain','lcc','ND461..ND497@5',0);
INSERT INTO concept VALUES('bokheim:painting-history:japan','Japan');
INSERT INTO concept_parent VALUES('bokheim:painting-history:japan','bokheim:lcc-detail:c9f19200-59d5-515d-a637-75729bb5f02e',0,6);
INSERT INTO source_selector VALUES('bokheim:painting-history:japan','lcc','ND1050..ND1059.6@5',0);
COMMIT;
