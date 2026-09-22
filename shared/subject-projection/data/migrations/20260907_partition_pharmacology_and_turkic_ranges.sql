-- Specific pharmacological systems and Turkish literature move down as broad
-- schedule ranges. General pharmacology and Turkic-language material remain.
BEGIN;

DELETE FROM lcc_range WHERE concept_id=2893;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('RM300','RM302.99',2893),('RM303','RM309',8593),
 ('RM309.1','RM314.99',2893),('RM315','RM334',9757),
 ('RM334.1','RM344.99',2893),('RM345','RM349',9054),
 ('RM349.1','RM354.99',2893),('RM355','RM365',9851),
 ('RM365.1','RM369.99',2893),('RM370','RM373',8194),
 ('RM373.1','RM374.99',2893),('RM375','RM377',9309),
 ('RM377.1','RM379.99',2893),('RM380','RM386',8891),
 ('RM386.1','RM387.99',2893),('RM388','RM388.7',7943),
 ('RM388.71','RM411.99',2893),('RM412','RM413',7106),
 ('RM413.1','RM666',2893);

DELETE FROM lcc_range WHERE concept_id=6797
 AND start_code='PL21' AND end_code='PL396';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('PL21','PL200.99',6797),('PL201','PL272',12532),
 ('PL272.1','PL396',6797);

COMMIT;
