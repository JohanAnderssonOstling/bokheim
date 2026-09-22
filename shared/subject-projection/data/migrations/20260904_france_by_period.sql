-- Present French history through one flat period index. Existing detail below
-- each retained period remains intact for later curation.

PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

INSERT INTO concept(concept_id,preferred_label)
VALUES ('bokheim:france-history:by-period','By period');
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
VALUES ('bokheim:france-history:by-period','unified:3754b57e-8d6a-5d89-b8bd-cedbb34ef143',0,10);

CREATE TEMP TABLE france_period(
    concept_id TEXT PRIMARY KEY,
    preferred_label TEXT NOT NULL,
    display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO france_period VALUES
    ('unified:743ac652-6402-5c66-b1ed-a3aff95fcb5a','Early and medieval, to 1515',10),
    ('unified:8e12f4e1-058c-5a8c-bcb6-a65df85b172f','16th century',20),
    ('bokheim:lcc-detail:e8b0733e-757c-5a7f-9ff0-f691db9ee982','17th century',30),
    ('bokheim:lcc-detail:878d9585-639b-5a71-8f2c-66c48191e600','18th century',40),
    ('bokheim:france-history:revolutionary-napoleonic-period','Revolutionary and Napoleonic period, 1789-1815',50),
    ('unified:233ec22c-cb7e-5a9e-a048-fa03bec5bf54','19th century',60),
    ('unified:1a6ed314-e0f9-5d5a-8202-b2830634b7b4','20th century',70),
    ('bokheim:france-history:21st-century','21st century',80);

DELETE FROM concept_parent
WHERE concept_id IN (SELECT concept_id FROM france_period);
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'bokheim:france-history:by-period',0,display_order
FROM france_period;

UPDATE concept
SET preferred_label=(SELECT preferred_label FROM france_period WHERE france_period.concept_id=concept.concept_id)
WHERE concept_id IN (SELECT concept_id FROM france_period);

-- The retained periods now replace this redundant chronological wrapper.
DELETE FROM concept_parent
WHERE concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2'
   OR parent_concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';
DELETE FROM concept WHERE concept_id='unified:9bfd6d9b-d5ef-559d-8310-8214e1b942a2';

DROP TABLE france_period;
COMMIT;
