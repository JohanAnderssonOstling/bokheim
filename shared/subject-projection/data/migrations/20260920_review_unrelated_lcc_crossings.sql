-- Reviewed against pinned 2025 schedule pages. See the accompanying review
-- report for all 55 pairs, source pages, and before/after matcher probes.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;
CREATE TEMP TABLE _crossing_moves (
 old_id INTEGER, old_start TEXT, old_end TEXT,
 new_id INTEGER, new_start TEXT, new_end TEXT
);
INSERT INTO _crossing_moves VALUES
 (13834,'DA660','DA669',13834,'DA660','DA664'),
 (51789,'DH511','DH519.Z',51789,'DH513','DH516'),
 (13946,'DK503.3','DK503.69.Z',11216,'DK503.3','DK503.69.Z'),
 (13950,'DK504.3','DK504.69.Z',11217,'DK504.3','DK504.69.Z'),
 (13954,'DK505.3','DK505.69.Z',11221,'DK505.3','DK505.69.Z'),
 (11390,'DK507.727','DK507.729',11390,'DK507.727','DK507.7293'),
 (11346,'DK507.729','DK507.73',11346,'DK507.7295','DK507.73'),
 (10352,'DQ113','DQ188.2',10352,'DQ113','DQ118'),
 (13860,'DU118','DU145',13860,'DU118','DU135'),
 (52572,'HV6571','HV6604',52572,'HV6571','HV6574'),
 (5710,'K1501','K1544',11741,'K1501','K1544'),
 (5710,'K1551','K1568',11741,'K1551','K1568'),
 (5710,'K1571','K1578.23',13314,'K1571','K1578.23'),
 (11749,'K3661','K3674',5299,'K3661','K3674'),
 (12990,'KB3817','KB38318',12990,'KB3817','KB3818'),
 (5319,'KB3790','KB4855',12990,'KB3790','KB4855'),
 (5619,'KBM1040','KBM11147',13246,'KBM1040','KBM1147'),
 (5562,'KIA313','KIA3317',51566,'KIA313','KIA320'),
 (5553,'KJC2260','KJC2666',5553,'KJC2260','KJC2266'),
 (5710,'KJC2655','KJC2700',13314,'KJC2655','KJC2700'),
 (13294,'KJC3449','KJC4439.9',13294,'KJC3449','KJC4353'),
 (13327,'KJC4420','KJC4447',5643,'KJC4420','KJC4428'),
 (13295,'KJC4440','KJC5599.9',13295,'KJC4431','KJC5599.9'),
 (2199,'LB1050','LB1099.99',2199,'LB1050.9','LB1099.99'),
 (2327,'LB1600','LB1704.99',2327,'LB1603','LB1704.99'),
 (7378,'PQ1600','PQ1713',7378,'PQ1600','PQ1709'),
 (13681,'PS80','PS96',13681,'PS80','PS81'),
 (9909,'R834','R837.8',9909,'R834','R837');

CREATE TEMP TABLE _crossing_guard(ok INTEGER CHECK(ok=1));
-- Refuse stale assumptions or a destination owned by an unexpected concept.
INSERT INTO _crossing_guard SELECT NOT EXISTS (
 SELECT 1 FROM _crossing_moves m WHERE NOT EXISTS (
  SELECT 1 FROM lcc_range r WHERE
   (r.concept_id=m.old_id AND r.start_code=m.old_start AND r.end_code=m.old_end)
   OR (r.concept_id=m.new_id AND r.start_code=m.new_start AND r.end_code=m.new_end)
 )
);
INSERT INTO _crossing_guard SELECT NOT EXISTS (
 SELECT 1 FROM _crossing_moves m JOIN lcc_range r
  ON r.start_code=m.new_start AND r.end_code=m.new_end
 WHERE r.concept_id<>m.new_id AND NOT (
  r.concept_id=m.old_id AND r.start_code=m.old_start AND r.end_code=m.old_end
 )
);
DELETE FROM lcc_range WHERE (concept_id,start_code,end_code)
 IN (SELECT old_id,old_start,old_end FROM _crossing_moves);
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
 SELECT new_start,new_end,new_id FROM _crossing_moves;
-- Abduction and kidnapping are separated by other offenses in the schedule.
INSERT OR IGNORE INTO lcc_range VALUES('HV6595','HV6604',52572);
-- Preserve the broader early-modern Swiss period around its corrected child.
INSERT OR IGNORE INTO lcc_range VALUES('DQ111','DQ123',10097);
-- Keep cultural detail while returning general history to its country.
INSERT OR IGNORE INTO lcc_range VALUES('DK503.3','DK503.36',13946);
INSERT OR IGNORE INTO lcc_range VALUES('DK504.3','DK504.36',13950);
INSERT OR IGNORE INTO lcc_range VALUES('DK505.3','DK505.36',13954);
-- Collective Belgian biographies retain both country and biography routes.
INSERT OR IGNORE INTO concept_parent VALUES(51789,3577,1);
-- DK507.73 is 1917-1945, not the Russian imperial period.
DELETE FROM lcc_selector WHERE concept_id=11390 AND selector='DK507.73';
INSERT OR IGNORE INTO lcc_selector VALUES(11346,'DK507.73');
-- KB is comparative religious law, not specifically canon law.
UPDATE concept SET preferred_label='Comparative Religious Criminal Law'
 WHERE concept_id=12990 AND preferred_label='Canon Criminal Law';
INSERT OR IGNORE INTO concept_parent VALUES(12990,5319,1);
-- Literary landmarks are a literary-history topic, not three literary forms.
DELETE FROM concept_parent WHERE concept_id=6614 AND parent_concept_id IN(3907,3910,3926);
INSERT OR IGNORE INTO concept_parent VALUES(6614,50826,0);
DROP TABLE _crossing_guard;
DROP TABLE _crossing_moves;
COMMIT;
