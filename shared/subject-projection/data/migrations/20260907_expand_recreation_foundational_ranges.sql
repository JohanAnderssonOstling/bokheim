BEGIN;

CREATE TEMP TABLE _split(label TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _split VALUES
 ('Recreation Reference & Study','GV1','GV14.999'),
 ('History of Recreation','GV15','GV158.999'),
 ('Leisure & Recreation in Society','GV159','GV181.349');

INSERT INTO concept(preferred_label)
SELECT label FROM _split s
WHERE NOT EXISTS (SELECT 1 FROM concept c WHERE c.preferred_label=s.label);

INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,4628,
       COALESCE((SELECT max(ordinal)+1 FROM concept_parent WHERE parent_concept_id=4628),0)
       + row_number() OVER (ORDER BY s.start_code)-1
FROM _split s JOIN concept c ON c.preferred_label=s.label
WHERE NOT EXISTS (SELECT 1 FROM concept_parent cp WHERE cp.concept_id=c.concept_id AND cp.parent_concept_id=4628);

INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT s.start_code,s.end_code,c.concept_id
FROM _split s JOIN concept c ON c.preferred_label=s.label;

DROP TABLE _split;
COMMIT;
