PRAGMA foreign_keys=ON;
BEGIN;

CREATE TABLE IF NOT EXISTS ddc_selector(
    selector TEXT NOT NULL CHECK(length(selector)=3 AND selector GLOB '[0-9][0-9][0-9]'),
    concept_id INTEGER NOT NULL REFERENCES concept(concept_id) ON DELETE CASCADE,
    PRIMARY KEY(selector)
) STRICT, WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS ddc_selector_concept_idx ON ddc_selector(concept_id);

-- First summary: attach only to existing curated subjects.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('000',4671),('100',4487),('200',4549),('300',5055),('400',3702),
 ('500',4779),('600',1),('700',510),('800',3846),('900',3319);

-- 010 bibliography sections. 013 is unassigned in DDC and is omitted.
INSERT OR REPLACE INTO ddc_selector(selector,concept_id) VALUES
 ('010',4678),('011',4718),('012',4765),('014',4713),('015',4756),
 ('016',4771),('017',4678),('018',4678),('019',4678);

COMMIT;
