-- Materialize documented literary spans as operational intervals. Keep the
-- original atomic selectors and all narrower author/topic rules as evidence.
-- Source record IDs refer to lcc-mdsconnect-2016.sqlite3 classification_record.
-- PR: https://www.loc.gov/aba/publications/FreeLCC/LCC_PR-PS_PZ2025TEXT.pdf
-- PQ: https://www.loc.gov/aba/publications/FreeLCC/LCC_PQ2025TEXT.pdf
-- PL: https://www.loc.gov/aba/publications/FreeLCC/LCC_PL-PM2025TEXT.pdf#page=197
BEGIN IMMEDIATE;

WITH reviewed(concept_id,start_letters,start_number,end_letters,end_number) AS (
    VALUES
        -- English literature (194721), French literature (260089),
        -- Spanish literature (325388), Portuguese literature (323278).
        (5585, 'PR', 1, 'PR', 9680),
        (4184, 'PQ', 1, 'PQ', 3999),
        (4226, 'PQ', 6001, 'PQ', 8929),
        (4215, 'PQ', 9000, 'PQ', 9999),
        -- The current Canadian scope extends the former PS8599 boundary
        -- through PS8649 (673739; 2025 schedule p. 544).
        (3867, 'PS', 8001, 'PS', 8649),
        -- English authors, 1900-1960 (196238), 1961-2000 (196627),
        -- and 2001 onward (6803). Existing author-specific rules still win.
        (4179, 'PR', 6000, 'PR', 6049),
        (4179, 'PR', 6050, 'PR', 6076),
        (4180, 'PR', 6100, 'PR', 6126),
        -- Collections of French literature (261001).
        (4295, 'PQ', 1100, 'PQ', 1297),
        -- Malaysian literature (440793), under existing Southeast Asian
        -- literature. Do not include neighboring language-study intervals.
        (4225, 'PL', 3512, 'PL', 3512.9)
)
INSERT INTO lcc_range(concept_id,start_letters,start_number,start_cutters,end_letters,end_number,end_cutters)
SELECT concept_id,start_letters,start_number,'[]',end_letters,end_number,'[]'
FROM reviewed r
WHERE NOT EXISTS (
    SELECT 1 FROM lcc_range existing
    WHERE existing.concept_id=r.concept_id
      AND existing.start_letters=r.start_letters AND existing.start_number=r.start_number
      AND existing.end_letters=r.end_letters AND existing.end_number=r.end_number
      AND existing.start_cutters='[]' AND existing.end_cutters='[]'
);

COMMIT;
