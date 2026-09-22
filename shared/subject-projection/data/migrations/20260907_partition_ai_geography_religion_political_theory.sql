-- Further high-use partitions using official, contiguous subject divisions.
-- General/reference gaps remain at their current parent.
BEGIN;

-- Artificial intelligence: computational intelligence is the final specific
-- division of the schedule; the preceding general AI block stays at AI.
DELETE FROM lcc_range WHERE concept_id=37
 AND start_code='Q334' AND end_code='Q342';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('Q334','Q341.99',37),('Q342','Q342',51697);

-- Geography: extract only its established broad subject divisions.
DELETE FROM lcc_range WHERE concept_id=2382
 AND start_code='G1' AND end_code='G922';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('G1','G64.99',2382),('G65','G69',2386),
 ('G70','G70.6',2391),('G70.61','G79.99',2382),
 ('G80','G99',2389),('G99.01','G100.49',2382),
 ('G100.5','G108.5',2394),('G108.51','G148.99',2382),
 ('G149','G180',2395),('G180.1','G539.99',2382),
 ('G540','G550',2392),('G550.1','G574.99',2382),
 ('G575','G890',2385),('G890.1','G904.99',2382),
 ('G905','G910',2396),('G910.1','G922',2382);

-- General religion remains BL1-549 and the later general gaps. Stable named
-- practice, institutional, and Hindu divisions move to existing children.
DELETE FROM lcc_range WHERE concept_id=4549
 AND start_code='BL1' AND end_code='BL2790';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('BL1','BL549.99',4549),('BL550','BL623.99',11640),
 ('BL624','BL629.99',11641),('BL630','BL659.99',5527),
 ('BL660','BL1099.99',4549),('BL1100','BL1295',4598),
 ('BL1295.1','BL2790',4549);

-- Political theory and the state. Official named divisions go to established
-- children; the small unspecialized gaps remain on the parent.
DELETE FROM lcc_range WHERE concept_id=5370
 AND start_code='JC11' AND end_code='JC607';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('JC11','JC50.9',13819),('JC51','JC93',12526),
 ('JC93.1','JC108.99',5370),('JC109','JC128.2',12527),
 ('JC128.3','JC130.99',5370),('JC131','JC275',12528),
 ('JC275.1','JC318.99',5370),('JC319','JC323',12529),
 ('JC323.1','JC326.99',5370),('JC327','JC347',13822),
 ('JC348','JC488.9',13823),('JC489','JC497',12530),
 ('JC497.1','JC500.99',5370),('JC501','JC570.9',13824),
 ('JC571','JC607',12531);
DELETE FROM lcc_selector WHERE
 (concept_id=12528 AND selector='JC131-275') OR
 (concept_id=12531 AND selector='JC571-607');

COMMIT;
