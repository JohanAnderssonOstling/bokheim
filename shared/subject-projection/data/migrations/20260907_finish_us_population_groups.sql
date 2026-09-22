BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
 (51235,'Puerto Rican History'),(51236,'Armenian American History'),
 (51237,'Ukrainian American History'),(51238,'Hungarian American History'),
 (51239,'Iranian American History'),(51240,'British American History'),
 (51241,'Scottish American History'),(51242,'Dutch American History'),
 (51243,'Czech American History'),(51244,'South Asian American History'),
 (51245,'French American History'),(51246,'Scots-Irish American History'),
 (51247,'Haitian American History'),(51248,'Lithuanian American History'),
 (51249,'Romanian American History'),(51250,'Danish American History'),
 (51251,'Finnish American History'),(51252,'European American History'),
 (51253,'Dominican American History'),(51254,'Scandinavian American History'),
 (51255,'Cambodian American History'),(51256,'Hmong American History'),
 (51257,'Swiss American History'),(51258,'Portuguese American History'),
 (51259,'Nigerian American History'),(51260,'Catholic American History'),
 (51261,'Palestinian American History'),(51262,'Turkish American History'),
 (51263,'Serbian American History'),(51264,'African Immigrant History'),
 (51265,'Slovak American History'),(51266,'West Indian American History'),
 (51267,'Salvadoran American History'),(51268,'Albanian American History'),
 (51269,'Welsh American History'),(51270,'Somali American History');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (51235,13702,0),(51236,13702,0),(51237,13702,0),(51238,13702,0),
 (51239,13702,0),(51240,13702,0),(51241,13702,0),(51242,13702,0),
 (51243,13702,0),(51244,13702,0),(51245,13702,0),(51246,13702,0),
 (51247,13702,0),(51248,13702,0),(51249,13702,0),(51250,13702,0),
 (51251,13702,0),(51252,13702,0),(51253,13702,0),(51254,13702,0),
 (51255,13702,0),(51256,13702,0),(51257,13702,0),(51258,13702,0),
 (51259,13702,0),(51260,13702,0),(51261,13702,0),(51262,13702,0),
 (51263,13702,0),(51264,13702,0),(51265,13702,0),(51266,13702,0),
 (51267,13702,0),(51268,13702,0),(51269,13702,0),(51270,13702,0);

INSERT INTO lcc_selector(concept_id,selector) VALUES
 (51235,'E184.P85'),(51236,'E184.A7'),(51237,'E184.U5'),
 (51238,'E184.H95'),(51239,'E184.I5'),(51240,'E184.B7'),
 (51241,'E184.S3'),(51242,'E184.D9'),(51243,'E184.B67'),
 (51244,'E184.S69'),(51245,'E184.F8'),(51246,'E184.S4'),
 (51247,'E184.H27'),(51248,'E184.L7'),(51249,'E184.R8'),
 (51250,'E184.S19'),(51251,'E184.F5'),(51252,'E184.E95'),
 (51253,'E184.D6'),(51254,'E184.S18'),(51255,'E184.K45'),
 (51256,'E184.H55'),(51257,'E184.S9'),(51258,'E184.P8'),
 (51259,'E184.N55'),(51260,'E184.C3'),(51261,'E184.P33'),
 (51262,'E184.T88'),(51263,'E184.S5'),(51264,'E184.A24'),
 (51265,'E184.S64'),(51266,'E184.W54'),(51267,'E184.S15'),
 (51268,'E184.A3'),(51269,'E184.W4'),(51270,'E184.S67');

-- Replace the permissive Cutter umbrella. Exact schedule headings with at
-- least five observed books remain directly on the parent when they are too
-- small or too general to warrant another visible subject. Codes below five
-- observed books are deliberately pruned.
DELETE FROM lcc_range
WHERE concept_id=13702 AND class_letters='E'
  AND start_value='184.A' AND end_value='184.Z';

INSERT INTO lcc_selector(concept_id,selector) VALUES
 (13702,'E184.A1'),(13702,'E184.35'),(13702,'E184.6'),
 (13702,'E184.36.P64'),(13702,'E184.36.E84'),(13702,'E184.36.S65'),
 (13702,'E184.65'),(13702,'E184.355'),(13702,'E184.36.W64'),
 (13702,'E184.36.A34'),(13702,'E184.S65'),(13702,'E184.B15'),
 (13702,'E184.354'),(13702,'E184.353'),(13702,'E184.T35'),
 (13702,'E184.L4'),(13702,'E184.P3'),(13702,'E184.B68'),
 (13702,'E184.P28'),(13702,'E184.C27'),(13702,'E184.S98'),
 (13702,'E184.L34'),(13702,'E184.E74'),(13702,'E184.C58'),
 (13702,'E184.W6'),(13702,'E184.S7'),(13702,'E184.J27'),
 (13702,'E184.H9'),(13702,'E184.E38'),(13702,'E184.F85'),
 (13702,'E184.E17'),(13702,'E184.352'),(13702,'E184.M44'),
 (13702,'E184.F89'),(13702,'E184.C34'),(13702,'E184.A23'),
 (13702,'E184.A2'),(13702,'E184.5'),(13702,'E184.Y7'),
 (13702,'E184.M8'),(13702,'E184.M3'),(13702,'E184.C93'),
 (13702,'E184.C2'),(13702,'E184.3512'),(13702,'E184.T4'),
 (13702,'E184.R85'),(13702,'E184.I43'),(13702,'E184.E7'),
 (13702,'E184.B8'),(13702,'E184.B13'),(13702,'E184.S77'),
 (13702,'E184.S695'),(13702,'E184.H66'),(13702,'E184.G82'),
 (13702,'E184.G27'),(13702,'E184.C24'),(13702,'E184.B89'),
 (13702,'E184.M47'),(13702,'E184.G84'),(13702,'E184.B26'),
 (13702,'E184.A4'),(13702,'E184.S55'),(13702,'E184.F894'),
 (13702,'E184.F57'),(13702,'E184.E28'),(13702,'E184.T53'),
 (13702,'E184.R3'),(13702,'E184.P35'),(13702,'E184.L53'),
 (13702,'E184.I7'),(13702,'E184.C4'),(13702,'E184.A9'),
 (13702,'E184.36.I58');

COMMIT;
