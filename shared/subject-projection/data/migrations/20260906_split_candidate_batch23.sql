PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- 50. Numismatics.
INSERT INTO concept VALUES ('bokheim:numismatics:general','Numismatics & Coins—General'),('bokheim:numismatics:ancient','Ancient Coins'),('bokheim:numismatics:later-coins','Medieval & Modern Coins'),('bokheim:numismatics:tokens','Tokens'),('bokheim:numismatics:medals','Medals & Medallions');
INSERT INTO concept_parent VALUES ('bokheim:numismatics:general','unified:c3f80b94-b0bc-591f-be31-ad9147c388de',0,100),('bokheim:numismatics:ancient','unified:c3f80b94-b0bc-591f-be31-ad9147c388de',0,200),('bokheim:numismatics:later-coins','unified:c3f80b94-b0bc-591f-be31-ad9147c388de',0,300),('bokheim:numismatics:tokens','unified:c3f80b94-b0bc-591f-be31-ad9147c388de',0,400),('bokheim:numismatics:medals','unified:c3f80b94-b0bc-591f-be31-ad9147c388de',0,500);
DELETE FROM source_selector WHERE concept_id='unified:c3f80b94-b0bc-591f-be31-ad9147c388de';
INSERT INTO source_selector VALUES ('bokheim:numismatics:general','lcc','CJ1..CJ200.9@4',0),('bokheim:numismatics:ancient','lcc','CJ201..CJ1399.9@4',0),('bokheim:numismatics:later-coins','lcc','CJ1400..CJ4625@4',0),('bokheim:numismatics:tokens','lcc','CJ4801..CJ5450@3',0),('bokheim:numismatics:medals','lcc','CJ5501..CJ6661@3',0);

COMMIT;
