-- Continue replacing high-hit whole-schedule fallbacks with specific child
-- subranges. General and uncovered intervals remain on the original parent.
BEGIN;

-- Building construction. Existing named children own the large official
-- divisions; the parent retains the unspecialized gaps between them.
DELETE FROM lcc_range WHERE concept_id=197
 AND ((start_code='TH' AND end_code='TH')
   OR (start_code='TH1' AND end_code='TH9745'));
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('TH1','TH437.99',197),('TH438.41','TH899.99',197),
 ('TH916','TH1150.99',197),('TH1156','TH1650.99',197),
 ('TH1676','TH2024.99',197),('TH3000.1','TH3300.99',197),
 ('TH3412','TH4020.99',197),('TH4978','TH5010.99',197),
 ('TH5701.3','TH6013.99',197),('TH6085.1','TH6100.99',197),
 ('TH6887.1','TH7004.99',197),('TH7976','TH8023.99',197),
 ('TH8032','TH8202.99',197),('TH8482','TH8580.99',197),
 ('TH8581.3','TH9745',197),('TH2025','TH3000',203);
DELETE FROM lcc_selector WHERE concept_id=203 AND selector='TH2025-3000';

-- Money and monetary policy. These established children form a complete,
-- mutually exclusive partition of the former HG201-1496 parent range.
DELETE FROM lcc_range WHERE concept_id=5192
 AND start_code='HG201' AND end_code='HG1496';
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id) VALUES
 ('HG201','HG230.29',12837),('HG230.3','HG230.3',52978),
 ('HG230.31','HG230.49',12837),('HG230.5','HG230.7',52979),
 ('HG230.71','HG257.99',12837),('HG258','HG320.9',12838),
 ('HG321','HG347.9',12839),('HG348','HG380.9',12840),
 ('HG381','HG450.9',12841),('HG451','HG1496',12842);
DELETE FROM lcc_selector WHERE concept_id=12837 AND selector='HG201-257.9';

-- Industry & Organization was already divided into explicit child blocks.
-- Remove only the redundant parent umbrellas; its Industry — General child
-- deliberately retains the true residual/general intervals.
DELETE FROM lcc_range WHERE concept_id=5233 AND
 ((start_code='HD2321' AND end_code='HD4730.9') OR
  (start_code='HD2340.782' AND end_code='HD2346.5') OR
  (start_code='HD2350.792' AND end_code='HD2356') OR
  (start_code='HD2709' AND end_code='HD2932'));

COMMIT;
