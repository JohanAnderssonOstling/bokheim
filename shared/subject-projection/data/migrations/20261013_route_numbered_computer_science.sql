-- QA75.5-76.95 is Electronic computers / Computer science in the LC Q outline:
-- https://www.loc.gov/catdir/cpso/lcco/lcco_q.pdf
-- The existing printed range selector matches the range label only. Add its
-- numeric coverage so individual calls such as QA76.76 do not fall through to
-- Mathematics / History, Education & Reference (QA1..QA99). More specific
-- computing selectors, including QA76.76.D47 (Software Engineering), still win.
BEGIN IMMEDIATE;

INSERT OR IGNORE INTO lcc_range
    (concept_id,start_letters,start_number,start_cutters,end_letters,end_number,end_cutters)
SELECT concept_id,'QA',75.5,'[]','QA',76.95,'[]'
FROM concept
WHERE concept_id=62 AND preferred_label='Computer Science';

COMMIT;
