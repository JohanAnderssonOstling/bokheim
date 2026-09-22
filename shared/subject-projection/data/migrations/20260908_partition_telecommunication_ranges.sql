-- Partition Telecommunication into official subject spans. General theory and
-- uncovered gaps remain at Telecommunication; no exact-code enumeration.
BEGIN;

DELETE FROM lcc_range WHERE concept_id=263;
DELETE FROM lcc_range WHERE concept_id=9935
 AND start_code='TK5103' AND end_code='TK5103.8';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TK5101','TK5102.9199',263),('TK5102.92','TK5102.98',7887),
 ('TK5102.9801','TK5102.9999',263),
 ('TK5103','TK5103.1999',9935),('TK5103.2','TK5103.49',6205),
 ('TK5103.4901','TK5103.5899',9935),('TK5103.59','TK5103.592',6619),
 ('TK5103.5921','TK5103.6999',9935),('TK5103.7','TK5103.78',8734),
 ('TK5103.7801','TK5103.8',9935),('TK5103.8001','TK5103.9999',263),
 ('TK5104','TK5104.2',7823),('TK5104.21','TK5104.9999',263),
 ('TK5105','TK5105.45',9162),('TK5105.4501','TK5105.4999',263);

-- Computer Networks already owns a mutually partitioned TK5105.5-.9 block.
-- Its child ranges remain unchanged.
COMMIT;
