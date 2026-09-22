-- Collapse approved navigation wrappers; preserve surviving IDs and selectors.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE collapse_guard(ok INTEGER CHECK(ok=1));
-- Special types of environment -> Environmental
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=308 AND preferred_label='Special types of environment';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=308;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=308;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=308;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=308 AND parent_concept_id=295 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=8962 AND parent_concept_id=308 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=295 WHERE concept_id=8962 AND parent_concept_id=308;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=9136 AND parent_concept_id=308 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=295 WHERE concept_id=9136 AND parent_concept_id=308;
DELETE FROM concept WHERE concept_id=308;
-- Adventure & Lost Treasure -> Geography
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=2383 AND preferred_label='Adventure & Lost Treasure';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=2383;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=2383;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=2383;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=2383 AND parent_concept_id=2382 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=9466 AND parent_concept_id=2383 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2382 WHERE concept_id=9466 AND parent_concept_id=2383;
DELETE FROM concept WHERE concept_id=2383;
-- Perioperative Care -> General & Operative Surgery
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=2882 AND preferred_label='Perioperative Care';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=2882;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=2882;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=2882;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=2882 AND parent_concept_id=5784 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6726 AND parent_concept_id=2882 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5784 WHERE concept_id=6726 AND parent_concept_id=2882;
DELETE FROM concept WHERE concept_id=2882;
-- English fiction -> English & Old English Literatures
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=3908 AND preferred_label='English fiction';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=3908;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=3908;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3908;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3908 AND parent_concept_id=3911 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=7801 AND parent_concept_id=3908 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=3911 WHERE concept_id=7801 AND parent_concept_id=3908;
DELETE FROM concept WHERE concept_id=3908;
-- Philosophy & Social Aspects -> History, Philosophy & Society
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=4880 AND preferred_label='Philosophy & Social Aspects';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=4880;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=4880;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=4880;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=4880 AND parent_concept_id=5725 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13323 AND parent_concept_id=4880 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5725 WHERE concept_id=13323 AND parent_concept_id=4880;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13324 AND parent_concept_id=4880 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5725 WHERE concept_id=13324 AND parent_concept_id=4880;
DELETE FROM concept WHERE concept_id=4880;
-- IP, Competition & Consumer Law -> Business & Commercial Law
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5616 AND preferred_label='IP, Competition & Consumer Law';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5616;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5616;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5616;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5616 AND parent_concept_id=5551 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13314 AND parent_concept_id=5616 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5551 WHERE concept_id=13314 AND parent_concept_id=5616;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13315 AND parent_concept_id=5616 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5551 WHERE concept_id=13315 AND parent_concept_id=5616;
DELETE FROM concept WHERE concept_id=5616;
-- Industry & Recording -> Music
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5658 AND preferred_label='Industry & Recording';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5658;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5658;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5658;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5658 AND parent_concept_id=720 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=724 AND parent_concept_id=5658 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=720 WHERE concept_id=724 AND parent_concept_id=5658;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=838 AND parent_concept_id=5658 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=720 WHERE concept_id=838 AND parent_concept_id=5658;
DELETE FROM concept WHERE concept_id=5658;
-- Prehistoric & Mythical Creatures -> Animals
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5777 AND preferred_label='Prehistoric & Mythical Creatures';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5777;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5777;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5777;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5777 AND parent_concept_id=1100 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=1109 AND parent_concept_id=5777 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=1100 WHERE concept_id=1109 AND parent_concept_id=5777;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=1111 AND parent_concept_id=5777 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=1100 WHERE concept_id=1111 AND parent_concept_id=5777;
DELETE FROM concept WHERE concept_id=5777;
-- Global & Cultural Traditions -> Regional & Ethnic
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5796 AND preferred_label='Global & Cultural Traditions';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5796;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5796;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5796;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5796 AND parent_concept_id=2980 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3006 AND parent_concept_id=5796 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2980 WHERE concept_id=3006 AND parent_concept_id=5796;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3010 AND parent_concept_id=5796 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2980 WHERE concept_id=3010 AND parent_concept_id=5796;
DELETE FROM concept WHERE concept_id=5796;
-- General & Applied Biology -> Biology & Life Sciences
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5829 AND preferred_label='General & Applied Biology';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5829;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5829;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5829;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5829 AND parent_concept_id=4789 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=4792 AND parent_concept_id=5829 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4789 WHERE concept_id=4792 AND parent_concept_id=5829;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13696 AND parent_concept_id=5829 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4789 WHERE concept_id=13696 AND parent_concept_id=5829;
DELETE FROM concept WHERE concept_id=5829;
-- Study Skills & Test Guides -> Study Aids
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5847 AND preferred_label='Study Skills & Test Guides';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5847;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5847;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5847;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5847 AND parent_concept_id=2335 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=2363 AND parent_concept_id=5847 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2335 WHERE concept_id=2363 AND parent_concept_id=5847;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=2365 AND parent_concept_id=5847 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2335 WHERE concept_id=2365 AND parent_concept_id=5847;
DELETE FROM concept WHERE concept_id=5847;
-- Safety & Sustainable Living -> House & Home
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=5924 AND preferred_label='Safety & Sustainable Living';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=5924;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=5924;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5924;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=5924 AND parent_concept_id=3125 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3144 AND parent_concept_id=5924 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=3125 WHERE concept_id=3144 AND parent_concept_id=5924;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3147 AND parent_concept_id=5924 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=3125 WHERE concept_id=3147 AND parent_concept_id=5924;
DELETE FROM concept WHERE concept_id=5924;
-- Personal & Mental Health -> Public Health
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=6158 AND preferred_label='Personal & Mental Health';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=6158;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=6158;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6158;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6158 AND parent_concept_id=2845 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6194 AND parent_concept_id=6158 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2845 WHERE concept_id=6194 AND parent_concept_id=6158;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6195 AND parent_concept_id=6158 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2845 WHERE concept_id=6195 AND parent_concept_id=6158;
DELETE FROM concept WHERE concept_id=6158;
-- Early-Childhood & Preschool Curricula -> Early Childhood Education
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=6691 AND preferred_label='Early-Childhood & Preschool Curricula';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=6691;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=6691;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6691;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=6691 AND parent_concept_id=2317 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53164 AND parent_concept_id=6691 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2317 WHERE concept_id=53164 AND parent_concept_id=6691;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53165 AND parent_concept_id=6691 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=2317 WHERE concept_id=53165 AND parent_concept_id=6691;
DELETE FROM concept WHERE concept_id=6691;
-- Sex role. Sex differences -> Sexual Psychology & Behavior
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=8640 AND preferred_label='Sex role. Sex differences';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=8640;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=8640;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=8640;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=8640 AND parent_concept_id=5433 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=11090 AND parent_concept_id=8640 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5433 WHERE concept_id=11090 AND parent_concept_id=8640;
DELETE FROM concept WHERE concept_id=8640;
-- Form perception -> Psychology
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=10779 AND preferred_label='Form perception';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=10779;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE parent_concept_id=10779;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=10779;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=10779 AND parent_concept_id=5381 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=10829 AND parent_concept_id=10779 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5381 WHERE concept_id=10829 AND parent_concept_id=10779;
DELETE FROM concept WHERE concept_id=10779;
-- Computer Performance -> Computer Systems & Hardware
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=11713 AND preferred_label='Computer Performance';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=11713;
INSERT INTO collapse_guard SELECT count(*)=2 FROM concept_parent WHERE parent_concept_id=11713;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=11713;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=11713 AND parent_concept_id=5574 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53197 AND parent_concept_id=11713 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5574 WHERE concept_id=53197 AND parent_concept_id=11713;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53198 AND parent_concept_id=11713 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5574 WHERE concept_id=53198 AND parent_concept_id=11713;
DELETE FROM concept WHERE concept_id=11713;
-- Number-Theory Arithmetic & Geometry -> Number Theory
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=13332 AND preferred_label='Number-Theory Arithmetic & Geometry';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=13332;
INSERT INTO collapse_guard SELECT count(*)=3 FROM concept_parent WHERE parent_concept_id=13332;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13332;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=13332 AND parent_concept_id=4956 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53199 AND parent_concept_id=13332 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4956 WHERE concept_id=53199 AND parent_concept_id=13332;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53200 AND parent_concept_id=13332 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4956 WHERE concept_id=53200 AND parent_concept_id=13332;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=53201 AND parent_concept_id=13332 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=4956 WHERE concept_id=53201 AND parent_concept_id=13332;
DELETE FROM concept WHERE concept_id=13332;
-- U.S. Biographical Writing -> U.S. Life Writing
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept WHERE concept_id=3855 AND preferred_label='U.S. Biographical Writing';
INSERT INTO collapse_guard SELECT count(*)=0 FROM (SELECT concept_id FROM lcc_selector UNION ALL SELECT concept_id FROM bisac_selector UNION ALL SELECT concept_id FROM lcc_range) WHERE concept_id=3855;
INSERT INTO collapse_guard SELECT count(*)=3 FROM concept_parent WHERE parent_concept_id=3855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=3855 AND parent_concept_id=5860 AND ordinal=0;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=7799 AND parent_concept_id=3855 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5860 WHERE concept_id=7799 AND parent_concept_id=3855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=8454 AND parent_concept_id=3855 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5860 WHERE concept_id=8454 AND parent_concept_id=3855;
INSERT INTO collapse_guard SELECT count(*)=1 FROM concept_parent WHERE concept_id=9008 AND parent_concept_id=3855 AND ordinal=0;
UPDATE concept_parent SET parent_concept_id=5860 WHERE concept_id=9008 AND parent_concept_id=3855;
DELETE FROM concept WHERE concept_id=3855;
DROP TABLE collapse_guard;
COMMIT;
