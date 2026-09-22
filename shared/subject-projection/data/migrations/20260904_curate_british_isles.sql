-- Separate British Isles countries from shared historical periods.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
INSERT OR IGNORE INTO concept VALUES('bokheim:british-isles:by-period','By period');

CREATE TEMP TABLE bi_move(source_id TEXT,target_id TEXT,system_id TEXT,selector TEXT,PRIMARY KEY(source_id,system_id,selector)) WITHOUT ROWID;
INSERT INTO bi_move
SELECT s.concept_id,'unified:4d2bc441-8fa4-5b72-bfc6-6ebf7772f353',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='unified:50dedc69-a57f-5894-8bed-df4eaacf67f6'
UNION ALL SELECT s.concept_id,'bokheim:british-isles:by-period',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='unified:b3c33fdb-084e-5c25-bf1d-38bf3c9b2c15'
UNION ALL SELECT s.concept_id,'unified:890b432d-05e5-5b96-9ffe-6d21b5dc4f3b',s.system_id,s.selector FROM source_selector s WHERE s.concept_id='bokheim:lcc-detail:26433245-2a24-5eb8-8713-01281dd07bfb';
WITH numbered AS (SELECT m.*,row_number() OVER(PARTITION BY target_id,system_id ORDER BY selector)-1 rn FROM bi_move m WHERE NOT EXISTS (SELECT 1 FROM source_selector s WHERE s.concept_id=m.target_id AND s.system_id=m.system_id AND s.selector=m.selector))
INSERT OR IGNORE INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT target_id,system_id,selector,COALESCE((SELECT max(s.ordinal)+1 FROM source_selector s WHERE s.concept_id=numbered.target_id AND s.system_id=numbered.system_id),0)+rn FROM numbered;

DELETE FROM concept_parent WHERE concept_id IN ('unified:2554ca80-df26-57a6-99e3-e3ca2c026b0b','unified:4361d2b6-7ad3-5852-9791-19050823f1bb','unified:370efced-9ee3-5bff-9bd2-a75dba9062f7','unified:f196a9b7-eca9-5403-a7ee-9016bbf200a8','unified:f99ed91c-828e-51f3-a2ff-4165515be218','unified:ae74586c-2dc9-5d6d-8602-598c780c1a1a','unified:4d2bc441-8fa4-5b72-bfc6-6ebf7772f353','unified:184535e0-d007-5e9c-852b-44b33b2836cc','unified:b3c33fdb-084e-5c25-bf1d-38bf3c9b2c15','unified:50dedc69-a57f-5894-8bed-df4eaacf67f6') AND parent_concept_id='unified:f3fa3554-e35d-512f-9115-0052e24458fc';
DELETE FROM concept_parent WHERE concept_id='bokheim:lcc-detail:26433245-2a24-5eb8-8713-01281dd07bfb' OR parent_concept_id IN ('unified:b3c33fdb-084e-5c25-bf1d-38bf3c9b2c15','unified:50dedc69-a57f-5894-8bed-df4eaacf67f6');
DELETE FROM concept WHERE concept_id IN ('unified:b3c33fdb-084e-5c25-bf1d-38bf3c9b2c15','unified:50dedc69-a57f-5894-8bed-df4eaacf67f6','bokheim:lcc-detail:26433245-2a24-5eb8-8713-01281dd07bfb')
AND NOT EXISTS (SELECT 1 FROM concept_parent p WHERE p.concept_id=concept.concept_id);

INSERT INTO concept_parent VALUES('bokheim:british-isles:by-period','unified:f3fa3554-e35d-512f-9115-0052e24458fc',0,20);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order) VALUES
('unified:f196a9b7-eca9-5403-a7ee-9016bbf200a8','bokheim:british-isles:by-period',0,10),
('unified:f99ed91c-828e-51f3-a2ff-4165515be218','bokheim:british-isles:by-period',0,20),
('unified:4d2bc441-8fa4-5b72-bfc6-6ebf7772f353','bokheim:british-isles:by-period',0,30),
('unified:ae74586c-2dc9-5d6d-8602-598c780c1a1a','bokheim:british-isles:by-period',0,40),
('unified:370efced-9ee3-5bff-9bd2-a75dba9062f7','bokheim:british-isles:by-period',0,50),
('unified:184535e0-d007-5e9c-852b-44b33b2836cc','bokheim:british-isles:by-period',0,60),
('unified:2554ca80-df26-57a6-99e3-e3ca2c026b0b','bokheim:british-isles:by-period',0,70),
('unified:4361d2b6-7ad3-5852-9791-19050823f1bb','bokheim:british-isles:by-period',0,80);
UPDATE concept SET preferred_label='British Empire & Commonwealth' WHERE concept_id='unified:890b432d-05e5-5b96-9ffe-6d21b5dc4f3b';
UPDATE concept SET preferred_label='Middle Ages' WHERE concept_id='unified:f196a9b7-eca9-5403-a7ee-9016bbf200a8';
UPDATE concept SET preferred_label='Norman & Late Medieval Era' WHERE concept_id='unified:f99ed91c-828e-51f3-a2ff-4165515be218';
UPDATE concept SET preferred_label='Tudor & Elizabethan Era, 1485–1603' WHERE concept_id='unified:4d2bc441-8fa4-5b72-bfc6-6ebf7772f353';
UPDATE concept SET preferred_label='Stuart Era, 1603–1714' WHERE concept_id='unified:ae74586c-2dc9-5d6d-8602-598c780c1a1a';
UPDATE concept SET preferred_label='Georgian Era, 1714–1837' WHERE concept_id='unified:370efced-9ee3-5bff-9bd2-a75dba9062f7';
UPDATE concept SET preferred_label='Victorian Era, 1837–1901' WHERE concept_id='unified:184535e0-d007-5e9c-852b-44b33b2836cc';
DROP TABLE bi_move;
COMMIT;
