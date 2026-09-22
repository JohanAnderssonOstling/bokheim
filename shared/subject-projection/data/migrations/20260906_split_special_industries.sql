PRAGMA foreign_keys=ON;
BEGIN IMMEDIATE;

-- HD9000-HD9999 contains distinct industry families. Reuse the imported leaf
-- for food and replace its overlapping selector set with the official range.
UPDATE concept SET preferred_label='Food Industry & Trade'
WHERE concept_id='unified:d5c35eeb-7ec5-5d9b-b24c-6f1e80251e3c';
DELETE FROM source_selector
WHERE concept_id='unified:d5c35eeb-7ec5-5d9b-b24c-6f1e80251e3c';
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
VALUES('unified:d5c35eeb-7ec5-5d9b-b24c-6f1e80251e3c','lcc','HD9000..HD9495@3',0);

CREATE TEMP TABLE industry_split(
 concept_id TEXT PRIMARY KEY,
 label TEXT NOT NULL,
 selector TEXT NOT NULL UNIQUE,
 display_order INTEGER NOT NULL
) WITHOUT ROWID;
INSERT INTO industry_split VALUES
('bokheim:industries:energy','Energy Industries','HD9502..HD9502.5@3',200),
('bokheim:industries:mineral','Mineral & Extractive Industries','HD9506..HD9624@3',210),
('bokheim:industries:chemical','Chemical Industry','HD9650..HD9663@3',220),
('bokheim:industries:pharmaceutical','Pharmaceutical Industry','HD9665..HD9675@3',230),
('bokheim:industries:specialized-technology','Specialized Technology Industries','HD9676..HD9678@3',240),
('bokheim:industries:machinery-equipment','Machinery & Equipment Industries','HD9680..HD9714.2@3',250),
('bokheim:industries:construction-materials','Construction & Building Materials Industries','HD9715..HD9718.5@3',260),
('bokheim:industries:manufacturing','Manufacturing Industries','HD9720..HD9975@3',270),
('bokheim:industries:services','Service Industries','HD9980..HD9990@3',280),
('bokheim:industries:miscellaneous','Miscellaneous Specialized Industries','HD9992..HD9999@3',290);

INSERT INTO concept(concept_id,preferred_label)
SELECT concept_id,label FROM industry_split;
INSERT INTO concept_parent(concept_id,parent_concept_id,ordinal,display_order)
SELECT concept_id,'unified:9ccac193-2002-5554-ab6f-076977b5f112',0,display_order
FROM industry_split;
INSERT INTO source_selector(concept_id,system_id,selector,ordinal)
SELECT concept_id,'lcc',selector,0 FROM industry_split;

COMMIT;
