-- Route the official family/marriage/home schedule to the existing dedicated
-- sibling subtree. The broader HQ range remains a fallback for gaps and Women.

INSERT INTO lcc_range(start_code, end_code, concept_id)
VALUES ('HQ503', 'HQ1064', 5252);

INSERT OR IGNORE INTO lcc_selector(concept_id, selector)
VALUES (5252, 'HQ503-1064');
