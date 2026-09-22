-- Published historic Sociology and Canadian alternate schedules.
-- Sources: LOC Sociology policy; LAC Class PS8000, fifth edition (2023).
BEGIN IMMEDIATE;
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('HM1','HM299',5488);
-- PS8000 is Canadian literature regardless of language, not exclusively English.
DELETE FROM lcc_range WHERE start_code='PS8001' AND end_code='PS8599' AND concept_id=3867;
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('PS8001','PS8649',4161);
-- LAC documents the PS9000 convention for French Canadian literature.
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('PS9001','PS9649',12697);
-- Reference hierarchy places the pre-Christian period beneath Jewish philosophy.
UPDATE concept_parent SET parent_concept_id=11088 WHERE concept_id=11132 AND parent_concept_id=10995;
COMMIT;
