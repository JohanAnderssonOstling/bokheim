-- These J ranges describe general legislative/executive papers, not the JF-JS
-- government-administration subtree.  They overlap one another, observed
-- specific use is below the curation threshold, and many observed J-number
-- strings are not credible LCC data.  Retain the valid exact JF headings.

DELETE FROM lcc_range
WHERE concept_id=5691
  AND (start_code,end_code) IN (
    ('J1','J981'),
    ('J80','J82'),
    ('J83','J98')
  );
