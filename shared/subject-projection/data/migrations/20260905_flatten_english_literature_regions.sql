-- Flatten English regional literature under English > By Region.
-- Reuse Canadian and American subjects; retain general overseas codes on By Region.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label='By Region' WHERE concept_id='unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63';
UPDATE concept_parent SET parent_concept_id='bokheim:subject:literature:english',display_order=10 WHERE concept_id='unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63';
UPDATE concept_parent SET parent_concept_id='unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63' WHERE concept_id IN ('unified:41899964-8fb9-5d37-8981-6330055f2964','unified:ceb2d443-22cd-5b2e-af87-9f759bf6fbaa');
UPDATE concept SET preferred_label='American' WHERE concept_id='unified:ceb2d443-22cd-5b2e-af87-9f759bf6fbaa';
UPDATE source_selector SET concept_id='unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63' WHERE concept_id='bokheim:lcc-detail:c0b4f5b0-746b-5bdc-b3d9-9e74e59cd1a0';
DELETE FROM concept WHERE concept_id='bokheim:lcc-detail:c0b4f5b0-746b-5bdc-b3d9-9e74e59cd1a0';
UPDATE concept SET preferred_label='Canadian' WHERE concept_id='unified:41899964-8fb9-5d37-8981-6330055f2964';
INSERT INTO source_selector VALUES('unified:41899964-8fb9-5d37-8981-6330055f2964','lcc','PR9180..PR9199.4@5',1);
INSERT INTO concept VALUES('bokheim:english-literature:oceania','Australian, New Zealand & Pacific');
INSERT INTO concept_parent VALUES('bokheim:english-literature:oceania','unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63',0,11);
INSERT INTO source_selector VALUES('bokheim:english-literature:oceania','lcc','PR9600..PR9670@5',0);
INSERT INTO concept VALUES('bokheim:english-literature:africa','African');
INSERT INTO concept_parent VALUES('bokheim:english-literature:africa','unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63',0,12);
INSERT INTO source_selector VALUES('bokheim:english-literature:africa','lcc','PR9340..PR9408@5',0);
INSERT INTO concept VALUES('bokheim:english-literature:asia','Asian');
INSERT INTO concept_parent VALUES('bokheim:english-literature:asia','unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63',0,13);
INSERT INTO source_selector VALUES('bokheim:english-literature:asia','lcc','PR9410..PR9570@5',0);
INSERT INTO concept VALUES('bokheim:english-literature:latin-america-caribbean','Caribbean & Latin American');
INSERT INTO concept_parent VALUES('bokheim:english-literature:latin-america-caribbean','unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63',0,14);
INSERT INTO source_selector VALUES('bokheim:english-literature:latin-america-caribbean','lcc','PR9200..PR9333.9@5',0);
INSERT INTO concept VALUES('bokheim:english-literature:other-europe','Other European');
INSERT INTO concept_parent VALUES('bokheim:english-literature:other-europe','unified:9fe6b484-cf1b-5f22-b2e7-d089d0a24b63',0,15);
INSERT INTO source_selector VALUES('bokheim:english-literature:other-europe','lcc','PR9090..PR9170@5',0);
COMMIT;
