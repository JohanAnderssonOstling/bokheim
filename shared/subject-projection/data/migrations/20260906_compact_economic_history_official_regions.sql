-- Replace HC country/enumeration selectors with the official broad regional
-- schedule spans and compact the coherent chronological/reference blocks.
PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

DELETE FROM source_selector
WHERE system_id='lcc' AND concept_id IN
 (11986,11987,11988,11989,11990,11991,11992,11993,13611,13613,13615);

INSERT INTO source_selector VALUES
 (11986,'lcc','HC95..HC120.5'),
 (11987,'lcc','HC121..HC239.5'),
 (11988,'lcc','HC240..HC407'),
 (11989,'lcc','HC411..HC495'),
 (11990,'lcc','HC800..HC1085'),
 (11991,'lcc','HC601..HC688.5'),
 (11992,'lcc','HC585..HC599'),
 (11993,'lcc','HC721..HC740'),
 (13611,'lcc','HC60..HC60.5'),
 (13613,'lcc','HC26..HC30.2'),
 (13615,'lcc','HC31..HC39');

CREATE TEMP TABLE _hc_base(id INTEGER NOT NULL);
INSERT INTO _hc_base SELECT COALESCE(MAX(concept_id),0) FROM concept;
INSERT INTO concept(concept_id,preferred_label)
SELECT id+1,'Communist Countries' FROM _hc_base;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT id+1,5091,COALESCE((SELECT MAX(ordinal)+1 FROM concept_parent WHERE parent_concept_id=5091),0)
FROM _hc_base;
INSERT INTO source_selector(concept_id,system_id,selector)
SELECT id+1,'lcc','HC701..HC710.Z' FROM _hc_base;

-- Parent entries are exact selectors, so numeric extraction safely identifies
-- those now owned by the child ranges. Keep the discontinuous true general
-- and topical HC entries on the nonleaf.
DELETE FROM source_selector
WHERE concept_id=5091 AND system_id='lcc' AND (
 (CAST(substr(selector,3) AS REAL) BETWEEN 26 AND 39) OR
 (CAST(substr(selector,3) AS REAL) BETWEEN 51 AND 60.5) OR
 (CAST(substr(selector,3) AS REAL) BETWEEN 95 AND 495) OR
 (CAST(substr(selector,3) AS REAL) BETWEEN 585 AND 688.5) OR
 (CAST(substr(selector,3) AS REAL) BETWEEN 701 AND 740) OR
 (CAST(substr(selector,3) AS REAL) BETWEEN 800 AND 1085)
);

DROP TABLE _hc_base;
COMMIT;
