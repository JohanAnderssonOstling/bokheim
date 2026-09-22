-- Every range on these non-leaf concepts is wholly contained by a range on
-- one of its direct children. Retain printed notations as exact selectors.
BEGIN;

CREATE TEMP TABLE _covered_parent(concept_id INTEGER PRIMARY KEY);
INSERT INTO _covered_parent VALUES
 (2293),  -- Types of education
 (4616),  -- Sacred Writings
 (4802),  -- Spermatophyta. Phanerogams
 (5059),  -- Cultural Traits
 (5061),  -- Sociocultural Anthropology
 (5071),  -- Prehistoric archaeology
 (5075),  -- Races
 (5231),  -- Agricultural Economics
 (5520),  -- Statistics
 (5756),  -- History, Culture & Sacred Places
 (5897),  -- Social Welfare
 (7540),  -- Immunologic diseases
 (10099), -- 17th century
 (10169), -- Eastern Africa
 (11492), -- Catholic Studies & Reference
 (11494); -- Catholic Worship & Devotion

INSERT OR IGNORE INTO lcc_selector(concept_id,selector)
SELECT r.concept_id,r.start_code||'-'||r.end_code
FROM lcc_range r JOIN _covered_parent p USING(concept_id);

DELETE FROM lcc_range
WHERE concept_id IN (SELECT concept_id FROM _covered_parent);

DROP TABLE _covered_parent;
COMMIT;
