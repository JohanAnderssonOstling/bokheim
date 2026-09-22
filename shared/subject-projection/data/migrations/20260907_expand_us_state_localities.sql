BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
 (50950,'Southern California'),(50951,'Northern California'),
 (50952,'California Regions & Counties'),(50953,'California Cities & Towns'),
 (50954,'San Diego'),(50955,'Sacramento'),
 (50956,'New York Regions & Counties'),(50957,'New York Cities & Towns'),(50958,'Brooklyn'),
 (50959,'Massachusetts Regions & Counties'),(50960,'Cape Cod'),(50961,'Massachusetts Cities & Towns'),
 (50962,'Florida Cities & Towns'),(50963,'Miami'),(50964,'Orlando'),
 (50965,'St. Augustine'),(50966,'Tampa'),(50967,'Key West'),
 (50968,'Louisiana Cities & Towns'),(50969,'New Orleans'),
 (50970,'Pennsylvania Regions & Counties'),(50971,'Pennsylvania Cities & Towns'),
 (50972,'Texas Regions & Counties'),(50973,'Texas Cities & Towns'),(50974,'Fort Worth'),
 (50975,'Illinois Cities & Towns');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
 (50950,3435,0),(50951,3435,0),(50952,3435,0),(50953,3435,0),
 (50954,50953,0),(50955,50953,0),
 (50956,3461,0),(50957,3461,0),(50958,50957,0),
 (50959,3450,0),(50960,50959,0),(50961,3450,0),
 (50962,3439,0),(50963,50962,0),(50964,50962,0),(50965,50962,0),(50966,50962,0),(50967,50962,0),
 (50968,3447,0),(50969,50968,0),
 (50970,3467,0),(50971,3467,0),
 (50972,3472,0),(50973,3472,0),(50974,50973,0),
 (50975,3442,0);

-- Existing major-city children now sit beneath their state's city grouping.
UPDATE concept_parent SET parent_concept_id=50953 WHERE concept_id IN(50922,50923) AND parent_concept_id=3435;
UPDATE concept_parent SET parent_concept_id=50973 WHERE concept_id IN(50924,50925,50926,50927) AND parent_concept_id=3472;
UPDATE concept_parent SET parent_concept_id=50971 WHERE concept_id=50930 AND parent_concept_id=3467;

INSERT INTO lcc_range VALUES('F','867','867.499999',50950),('F','867.5','867.999',50951);
UPDATE lcc_range SET concept_id=50952 WHERE concept_id=3435 AND class_letters='F' AND start_value='868.A' AND end_value='868.Z';
UPDATE lcc_range SET concept_id=50953 WHERE concept_id=3435 AND class_letters='F' AND start_value='869.A' AND end_value='869.Z';
INSERT INTO lcc_range VALUES('F','869.S22','869.S22',50954),('F','869.S12','869.S12',50955);

UPDATE lcc_range SET concept_id=50956 WHERE concept_id=3461 AND class_letters='F' AND start_value='127.A' AND end_value='127.Z';
UPDATE lcc_range SET concept_id=50957 WHERE concept_id=3461 AND class_letters='F' AND start_value='129.A' AND end_value='129.Z';
INSERT INTO lcc_range VALUES('F','129.B7','129.B7',50958);

UPDATE lcc_range SET concept_id=50959 WHERE concept_id=3450 AND class_letters='F' AND start_value='72.A' AND end_value='72.Z';
INSERT INTO lcc_range VALUES('F','72.C3','72.C3',50960);
UPDATE lcc_range SET concept_id=50961 WHERE concept_id=3450 AND class_letters='F' AND start_value='74.A' AND end_value='74.Z';

UPDATE lcc_range SET concept_id=50962 WHERE concept_id=3439 AND class_letters='F' AND start_value='319.A' AND end_value='319.Z';
INSERT INTO lcc_range VALUES
 ('F','319.M6','319.M6',50963),('F','319.O7','319.O7',50964),('F','319.S2','319.S2',50965),
 ('F','319.T2','319.T2',50966),('F','319.K4','319.K4',50967);

UPDATE lcc_range SET concept_id=50968 WHERE concept_id=3447 AND class_letters='F' AND start_value='379.A' AND end_value='379.Z';
UPDATE lcc_range SET concept_id=50969 WHERE concept_id=3447 AND class_letters='F' AND start_value='379.N5' AND end_value='379.N59';

UPDATE lcc_range SET concept_id=50970 WHERE concept_id=3467 AND class_letters='F' AND start_value='157.A' AND end_value='157.Z';
UPDATE lcc_range SET concept_id=50971 WHERE concept_id=3467 AND class_letters='F' AND start_value='159.A' AND end_value='159.Z';

UPDATE lcc_range SET concept_id=50972 WHERE concept_id=3472 AND class_letters='F' AND start_value='392.A' AND end_value='392.Z';
UPDATE lcc_range SET concept_id=50973 WHERE concept_id=3472 AND class_letters='F' AND start_value='394.A' AND end_value='394.Z';
INSERT INTO lcc_range VALUES('F','394.F7','394.F7',50974);

UPDATE lcc_range SET concept_id=50975 WHERE concept_id=3442 AND class_letters='F' AND start_value='549.A' AND end_value='549.Z';

COMMIT;
