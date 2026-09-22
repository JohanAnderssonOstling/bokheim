BEGIN;

UPDATE concept SET preferred_label='Economic Thought by Period' WHERE concept_id=13771;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector) VALUES
 (5087,'HF1351-1647'),(5087,'HF3000-4055'),
 (12094,'HC426-430'),(52692,'HN101-942.7'),(13771,'HB151-181');
DELETE FROM lcc_range WHERE concept_id IN (5087,12094,52692,13771);

CREATE TEMP TABLE _split(label TEXT,parent_id,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('International Economic Relations',5087,'HF1351','HF1647'),('Tariffs, Free Trade & Protectionism',5087,'HF1701','HF2701'),('U.S. Foreign Commerce',5087,'HF3000','HF3163'),('European Commerce',5087,'HF3491','HF3750.8'),('Commerce by Country',5087,'HF3221','HF4040.7'),
 ('Chinese Economy before 1644',12094,'HC427.6','HC427.6'),('Chinese Economy, 1644–1912',12094,'HC427.7','HC427.7'),('Chinese Economy, 1912–1949',12094,'HC427.8','HC427.8'),('Chinese Economy, 1949–1976',12094,'HC427.9','HC427.9'),('Chinese Economy, 1976–2000',12094,'HC427.92','HC427.92'),('Chinese Economy since 2000',12094,'HC427.95','HC427.95'),
 ('European Social Conditions',52692,'HN371','HN650.8'),
 ('Economic Thought before 1776',13771,'HB151','HB159'),('Classical Economic Thought',13771,'HB161','HB169'),('Modern Economic Thought',13771,'HB171','HB180');
INSERT INTO concept(preferred_label) SELECT DISTINCT s.label FROM _split s WHERE NOT EXISTS(SELECT 1 FROM concept c WHERE c.preferred_label=s.label);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal) SELECT c.concept_id,s.parent_id,0 FROM _split s JOIN concept c ON c.preferred_label=s.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) SELECT s.start_code,s.end_code,c.concept_id FROM _split s JOIN concept c ON c.preferred_label=s.label;
DROP TABLE _split;
COMMIT;
