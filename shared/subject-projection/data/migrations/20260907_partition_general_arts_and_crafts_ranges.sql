-- Partition the two remaining high-use arts schedules using broad, official
-- subject spans. General material stays at the parent; no Cutter enumeration
-- is introduced.
BEGIN;

-- Arts in general (NX).
DELETE FROM lcc_range WHERE concept_id=980;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('NX1','NX410.99',980),('NX411','NX415',983),
 ('NX415.1','NX649.99',980),('NX650.A','NX650.Z',986),
 ('NX651','NX652.99',980),('NX653.A','NX653.Z',986),
 ('NX654','NX699.99',980),('NX700','NX750',985),
 ('NX750.1','NX759.99',980),('NX760','NX770',981),
 ('NX770.1','NX797.99',980),('NX798','NX820',982);

-- Replace the former exact NX650/NX653 Cutter enumerations with the structural
-- subject ranges above. The bare/general NX650 and NX653 values stay at parent.
DELETE FROM lcc_selector WHERE concept_id=986
 AND (selector GLOB 'NX650.*' OR selector GLOB 'NX653.*');

-- Handicrafts and arts & crafts (TT).
DELETE FROM lcc_range WHERE concept_id=3063
 AND start_code='TT1' AND end_code='TT999';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TT1','TT160.99',3063),('TT161','TT170.7',3070),
 ('TT170.71','TT173.99',3063),('TT174','TT176.2',3064),
 ('TT176.21','TT179.99',3063),('TT180','TT200.22',3074),
 ('TT200.23','TT200.99',3063),('TT201','TT203',3068),
 ('TT203.1','TT204.99',3063),('TT205','TT214.3',3071),
 ('TT214.31','TT324.99',3063),('TT325','TT345',3072),
 ('TT345.1','TT386.99',3063),('TT387','TT410',3073),
 ('TT410.1','TT489.99',3063),('TT490','TT695',3065),
 ('TT695.1','TT696.99',3063),('TT697','TT949.99',3067),
 ('TT950','TT979',3066),('TT980','TT999',3069);

COMMIT;
