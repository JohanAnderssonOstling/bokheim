BEGIN;

UPDATE concept SET preferred_label='U.S. General Reference' WHERE concept_id=13698;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51139,'U.S. Historic Places'),
  (51140,'U.S. National & State Parks'),
  (51141,'U.S. Social Life & Customs'),
  (51142,'U.S. Geography'),
  (51143,'U.S. Civilization & Intellectual Life');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51139,13698,0),
  (51140,13698,0),
  (51141,13698,0),
  (51142,13698,0),
  (51143,13698,0);

DELETE FROM lcc_range
WHERE concept_id=13698 AND class_letters='E'
  AND start_value='151' AND end_value='169.13';

-- Periodicals, collections, gazetteers, directories, geographic names, and
-- general reference works remain on the parent.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','151','157.999',13698),
  ('E','159','159.999',51139),
  ('E','160','160.999',51140),
  ('E','161','161.299',51141),
  ('E','161.3','161.499',51142),
  ('E','169.1','169.13',51143);

-- Guidebooks and the complete historical description-and-travel schedule are
-- travel material, not history. E169 Cutter spans parse within 169-169.099.
INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','158','158.999',2521),
  ('E','161.5','169.099',2521);

COMMIT;
