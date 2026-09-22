-- Distinguish subjects moved from profession-specific branches into Biography.
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label = CASE concept_id
    WHEN 8281 THEN 'Printers'
    WHEN 8868 THEN 'Cartoonists'
    WHEN 9571 THEN 'Designers'
    WHEN 9873 THEN 'Commercial Artists'
END
WHERE concept_id IN (8281,8868,9571,9873) AND preferred_label = 'Collective';

-- Reuse the existing U.S. biography period container and preserve every child.
UPDATE concept_parent SET parent_concept_id = 51004
WHERE parent_concept_id = 54932;
DELETE FROM concept WHERE concept_id = 54932 AND preferred_label = 'By Period';
COMMIT;
