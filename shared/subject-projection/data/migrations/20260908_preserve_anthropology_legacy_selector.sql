BEGIN;

-- Anthropology Fields was only a redundant navigational wrapper. Preserve its
-- legacy catalogue selector on Anthropology while narrower child ranges win by
-- specificity.
INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (5056, 'GN1-890');

-- Open Library also contains literal range notations. Keep those exact forms
-- alongside the structured ranges so such records continue to resolve to the
-- same specific child.
INSERT OR IGNORE INTO lcc_selector(concept_id, selector) VALUES
 (5061, 'GN301-674'),
 (5070, 'GN49-298'),
 (5071, 'GN700-890'),
 (4804, 'QD71-142'),
 (4806, 'QD146-197'),
 (4807, 'QD241-441'),
 (4808, 'QD450-882'),
 (4805, 'QD901-999'),
 (12758, 'HG4501-4528.9'),
 (12768, 'HG4900-5999.9');

COMMIT;
