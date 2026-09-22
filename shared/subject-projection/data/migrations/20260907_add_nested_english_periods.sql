BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51432,'Roman Britain'),(51433,'Saxon England, 445–1066'),
  (51434,'Norman England, 1066–1154'),(51435,'Plantagenet England, 1154–1399'),
  (51436,'Lancastrian & Yorkist England, 1399–1485'),
  (51437,'Henry VII, 1485–1509'),(51438,'Henry VIII, 1509–1547'),
  (51439,'Edward VI & Mary I, 1547–1558'),(51440,'Elizabeth I, 1558–1603'),
  (51441,'Early Stuarts, 1603–1642'),
  (51442,'Civil War & Commonwealth, 1642–1660'),
  (51443,'Later Stuarts, 1660–1702'),
  (51444,'Anne to George II, 1702–1760'),(51445,'George III, 1760–1820'),
  (51446,'Regency & Early 19th Century, 1801–1837'),
  (51447,'Victorian England, 1837–1901'),
  (51448,'Edward VII, 1901–1910'),(51449,'George V, 1910–1936'),
  (51450,'George VI, 1937–1952'),(51451,'Elizabeth II, 1952–2022');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51432,10093,0),(51433,10093,0),
  (51434,10229,0),(51435,10229,0),(51436,10229,0),
  (51437,10149,0),(51438,10149,0),(51439,10149,0),(51440,10149,0),
  (51441,10099,0),(51442,10099,0),(51443,10099,0),
  (51444,10155,0),(51445,10155,0),
  (51446,10213,0),(51447,10213,0),
  (51448,10115,0),(51449,10115,0),(51450,10115,0),(51451,10115,0);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('DA','145','147.999',51432),('DA','150','162.999',51433),
  ('DA','190','199.999',51434),
  ('DA','200','209.999',51435),('DA','220','237.999',51435),
  ('DA','240','260.999',51436),
  ('DA','330','330.999',51437),('DA','331','339.999',51438),
  ('DA','340','347.999',51439),('DA','350','360.999',51440),
  ('DA','385','398.999',51441),('DA','400','429.999',51442),
  ('DA','430','463.999',51443),
  ('DA','480','503.999',51444),('DA','505','522.999',51445),
  ('DA','535','542.999',51446),('DA','550','565.999',51447),
  ('DA','567','570.999',51448),('DA','573','578.999',51449),
  ('DA','584','588.399',51450),('DA','590','592.999',51451);

-- Hadrian's Wall is a Roman-Britain subject as well as a historic place.
DELETE FROM concept_parent WHERE concept_id=51425 AND parent_concept_id=10093;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
VALUES(51425,51432,0);

COMMIT;
