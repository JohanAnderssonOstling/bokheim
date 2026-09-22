BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (50904,'Mississippi Valley & Middle West'),
  (50905,'Old Northwest'),
  (50906,'Great Lakes'),
  (50907,'Great Plains'),
  (50908,'Rocky Mountains & Yellowstone'),
  (50909,'New Southwest & Colorado River Region'),
  (50910,'Pacific States & Pacific Northwest'),
  (50911,'Washington, D.C.');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (50904,3420,0),
  (50905,3420,0),
  (50906,3420,0),
  (50907,3420,0),
  (50908,3423,0),
  (50909,3423,0),
  (50910,3423,0),
  (50911,3422,0);

UPDATE lcc_range SET concept_id=50904
WHERE concept_id=3420 AND class_letters='F'
  AND ((start_value='350.3' AND end_value='358.2')
    OR (start_value='358' AND end_value='358.2'));

UPDATE lcc_range SET concept_id=50905
WHERE concept_id=3420 AND class_letters='F'
  AND start_value='476' AND end_value='485';

UPDATE lcc_range SET concept_id=50906
WHERE concept_id=3420 AND class_letters='F'
  AND start_value='550.5' AND end_value='556';

UPDATE lcc_range SET concept_id=50907
WHERE concept_id=3420 AND class_letters='F'
  AND ((start_value='590.2' AND end_value='596')
    OR (start_value='596.2' AND end_value='596.3'));

UPDATE source_selector SET concept_id=50908
WHERE concept_id=3423 AND system_id='lcc'
  AND selector IN ('F718','F721','F722');

UPDATE lcc_range SET concept_id=50909
WHERE concept_id=3423 AND class_letters='F'
  AND start_value='785.15' AND end_value='790';

UPDATE source_selector SET concept_id=50910
WHERE concept_id=3423 AND system_id='lcc'
  AND selector IN ('F851.5','F851.7');

UPDATE lcc_range SET concept_id=50910
WHERE concept_id=3423 AND class_letters='F' AND (
  (start_value='850.5' AND end_value='855.2') OR
  (start_value='851.72' AND end_value='854.2') OR
  (start_value='855' AND end_value='855.2')
);

UPDATE lcc_range SET concept_id=50911
WHERE concept_id=3422 AND class_letters='F' AND (
  (start_value='191' AND end_value='205') OR
  (start_value='200' AND end_value='200.3') OR
  (start_value='201' AND end_value='201.3')
);

COMMIT;
