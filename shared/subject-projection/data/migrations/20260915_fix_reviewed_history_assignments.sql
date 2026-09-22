-- Confirmed corrections from 20260915_history_correctness_review.md.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- Egyptian antiquities, not Lesotho. The religious subdivisions belong to
-- the existing Egyptian Antiquities subject; pyramids to Ancient Egypt.
UPDATE lcc_range SET concept_id=3481
WHERE concept_id=3346 AND start_code='DT63' AND end_code='DT63.5';
UPDATE lcc_range SET concept_id=52296
WHERE concept_id=3346 AND (start_code,end_code) IN
  (('DT68','DT68.8'),('DT68.2','DT68.3'));
UPDATE lcc_range SET concept_id=3349
WHERE concept_id=3371 AND start_code='DT193' AND end_code='DT193.95';

-- Broad BISAC period codes must resolve to one inclusive subject.
DELETE FROM bisac_selector WHERE selector IN
 ('HIS036020','HIS036030','HIS036050','HIS036060','HIS036070','HIS059000');
INSERT INTO bisac_selector(concept_id,selector) VALUES
 (3409,'HIS036020'),(3424,'HIS036030'),(3408,'HIS036050'),
 (3405,'HIS036060'),(3406,'HIS036070'),(3515,'HIS059000');
INSERT INTO source_label(concept_id,label,ordinal)
SELECT concept_id,preferred_label,
 coalesce((SELECT max(ordinal)+1 FROM source_label WHERE concept_id=3424),0)
FROM concept WHERE concept_id=3424 AND preferred_label<>'Revolutionary Period, 1775–1800'
AND NOT EXISTS(SELECT 1 FROM source_label s WHERE s.concept_id=3424 AND s.label=concept.preferred_label);
UPDATE concept SET preferred_label='Revolutionary Period, 1775–1800' WHERE concept_id=3424;

-- Preserve coverage while removing the erroneous early-period destination.
UPDATE lcc_range SET end_code='DT633.999'
WHERE concept_id=54301 AND start_code='DT633' AND end_code='DT636.3';
-- E838–E840.8 is general later-twentieth-century material. Retain the
-- Kennedy/Johnson scope of 50990 from E841 onward.
UPDATE lcc_range SET concept_id=3413
WHERE concept_id=50990 AND start_code='E838' AND end_code='E851.999';
-- Upgrade the initial split preview while keeping continuous numeric coverage.
UPDATE lcc_range SET end_code='E851.999'
WHERE concept_id=3413 AND start_code='E838' AND end_code='E840.999';
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('E841','E851.999',50990)
ON CONFLICT(start_code,end_code) DO NOTHING;

UPDATE bisac_selector SET concept_id=3693 WHERE selector='HIS027210' AND concept_id=3411;
UPDATE bisac_selector SET concept_id=11573 WHERE selector='HIS050000' AND concept_id=3485;
UPDATE bisac_selector SET concept_id=3551 WHERE selector='HIS055000' AND concept_id=3633;
UPDATE ddc_selector SET concept_id=3321 WHERE selector='965' AND concept_id=3357;
UPDATE ddc_selector SET concept_id=3634 WHERE selector='953' AND concept_id=3633;
UPDATE ddc_selector SET concept_id=3609 WHERE selector='948' AND concept_id=3517;

-- Allocate IDs centrally; select existing matching children on a repeat run.
CREATE TEMP TABLE canadian_regions(label TEXT PRIMARY KEY, code TEXT NOT NULL, id INTEGER);
INSERT INTO canadian_regions(label,code) VALUES
 ('Atlantic Provinces','HIS006040'),('British Columbia','HIS006050'),
 ('Northern Territories','HIS006060'),('Ontario','HIS006070'),
 ('Prairie Provinces','HIS006080'),('Québec','HIS006090');
UPDATE canadian_regions SET id=(SELECT c.concept_id FROM concept c
 JOIN concept_parent cp USING(concept_id) WHERE cp.parent_concept_id=50842
 AND c.preferred_label=canadian_regions.label);
INSERT INTO concept(preferred_label)
SELECT r.label FROM canadian_regions r WHERE r.id IS NULL;
UPDATE canadian_regions SET id=(SELECT max(concept_id) FROM concept WHERE preferred_label=canadian_regions.label)
WHERE id IS NULL;
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id,50842,0 FROM canadian_regions;
UPDATE bisac_selector SET concept_id=r.id FROM canadian_regions r WHERE selector=r.code;

CREATE TEMP TABLE canadian_spans(label TEXT, lo REAL, hi REAL);
INSERT INTO canadian_spans VALUES
 ('Atlantic Provinces',1036,1049.7),('Atlantic Provinces',1121,1139),
 ('British Columbia',1086,1089.7),('Northern Territories',1060,1060.97),
 ('Northern Territories',1091,1110.5),('Northern Territories',1141,1145.2),
 ('Ontario',1056,1059.7),('Prairie Provinces',1061,1065),
 ('Prairie Provinces',1070,1080),('Québec',1051,1055);
UPDATE lcc_range SET concept_id=r.id
FROM canadian_spans s JOIN canadian_regions r USING(label)
WHERE concept_id=54625 AND start_code GLOB 'F[0-9]*' AND end_code GLOB 'F[0-9]*'
AND cast(substr(start_code,2) AS REAL)>=s.lo
AND cast(substr(end_code,2) AS REAL)<=s.hi;

DROP TABLE canadian_spans;
DROP TABLE canadian_regions;
COMMIT;
