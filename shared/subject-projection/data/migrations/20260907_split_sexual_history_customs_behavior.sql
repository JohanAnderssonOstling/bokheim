BEGIN;

DELETE FROM lcc_range WHERE concept_id=12400;
INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
VALUES (12400,'HQ12-30.7');

CREATE TEMP TABLE _sexual_topics(label TEXT,start_code TEXT,end_code TEXT);
INSERT INTO _sexual_topics VALUES
 ('History of Sexual Customs','HQ12','HQ18'),
 ('Sexual Customs by Period','HQ13','HQ16'),
 ('Sexologist Biographies','HQ18.3','HQ18.32'),
 ('Gender Identity','HQ18.55','HQ18.552'),
 ('Sexual Behavior & Attitudes','HQ19','HQ30.7'),
 ('Adolescent & Young-Adult Sexuality','HQ27','HQ27.5'),
 ('Adult Sexuality','HQ28','HQ29');

INSERT INTO concept(preferred_label)
SELECT label FROM _sexual_topics t
WHERE NOT EXISTS (
 SELECT 1 FROM concept c WHERE c.preferred_label=t.label
);
INSERT OR IGNORE INTO concept_parent(concept_id,parent_concept_id,ordinal)
SELECT c.concept_id,12400,0
FROM _sexual_topics t JOIN concept c ON c.preferred_label=t.label;
INSERT OR IGNORE INTO lcc_range(start_code,end_code,concept_id)
SELECT t.start_code,t.end_code,c.concept_id
FROM _sexual_topics t JOIN concept c ON c.preferred_label=t.label;
DROP TABLE _sexual_topics;

COMMIT;
