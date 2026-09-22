BEGIN;

INSERT INTO concept(concept_id,preferred_label) VALUES
  (51100,'Confederation Period, 1775–1789'),
  (51101,'Constitutional Period, 1789–1809'),
  (51102,'Washington Administrations, 1789–1797'),
  (51103,'John Adams Administration, 1797–1801'),
  (51104,'Jefferson Administrations, 1801–1809'),
  (51105,'Early 19th Century, 1809–1845'),
  (51106,'Madison Administrations, 1809–1817'),
  (51107,'Monroe Administrations, 1817–1825'),
  (51108,'John Quincy Adams Administration, 1825–1829'),
  (51109,'Jackson Administrations, 1829–1837'),
  (51110,'Van Buren Administration, 1837–1841'),
  (51111,'William Henry Harrison Administration, 1841'),
  (51112,'Tyler Administration, 1841–1845'),
  (51113,'Middle 19th Century, 1845–1861'),
  (51114,'Polk Administration, 1845–1849'),
  (51115,'Taylor Administration, 1849–1850'),
  (51116,'Fillmore Administration, 1850–1853'),
  (51117,'Pierce Administration, 1853–1857'),
  (51118,'Buchanan Administration, 1857–1861');

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (51100,3417,0),
  (51101,3417,0),
  (51102,51101,0),(51103,51101,0),(51104,51101,0),
  (51105,3417,0),
  (51106,51105,0),(51107,51105,0),(51108,51105,0),
  (51109,51105,0),(51110,51105,0),(51111,51105,0),(51112,51105,0),
  (51113,3417,0),
  (51114,51113,0),(51115,51113,0),(51116,51113,0),
  (51117,51113,0),(51118,51113,0);

-- Reuse existing subjects through polyhierarchy rather than creating duplicate
-- War of 1812, Mexican War, or Slavery nodes.
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal) VALUES
  (3693,51105,1),
  (3414,3417,2),
  (12240,3417,1);

-- Remove the umbrella. Every range below is mutually exclusive. Broad material
-- remains on either the Early Republic parent or a broad period parent. LCC
-- presidential-campaign numbers (E320, E330, E337, E349-E350, E370, E375,
-- E380, E383, E385, E390, E400, E420, E430, E435, E440) are intentionally
-- absent, and wars remain flattened leaves rather than battle/campaign trees.
DELETE FROM lcc_range
WHERE concept_id=3417 AND class_letters='E' AND start_value='300' AND end_value='453';

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','300','302.499',3417),
  ('E','302.5','309.999',51100),

  ('E','310','310.999',51101),
  ('E','337.5','337.999',51101),
  ('E','311','319.999',51102),
  ('E','321','329.999',51103),
  ('E','331','336.999',51104),

  ('E','338','340.999',51105),
  ('E','365','369.999',51105),
  ('E','384','384.999',51105),
  ('E','341','348.999',51106),
  ('E','371','374.999',51107),
  ('E','376','379.999',51108),
  ('E','381','382.999',51109),
  ('E','386','389.999',51110),
  ('E','391','395.999',51111),
  ('E','396','399.999',51112),

  ('E','415.3','415.999',51113),
  ('E','440.5','440.999',51113),
  ('E','416','419.999',51114),
  ('E','421','425.999',51115),
  ('E','426','429.999',51116),
  ('E','431','434.999',51117),
  ('E','436','439.999',51118);

INSERT INTO lcc_range(class_letters,start_value,end_value,concept_id) VALUES
  ('E','351','364.999',3693),
  ('E','441','453',12240);

COMMIT;
