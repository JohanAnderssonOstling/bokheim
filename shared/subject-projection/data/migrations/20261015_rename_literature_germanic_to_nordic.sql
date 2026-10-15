-- Rename the existing Nordic literary branch; retain IDs, edges and selectors.
BEGIN IMMEDIATE;
UPDATE concept SET preferred_label = 'Nordic', normalized_label = 'nordic'
WHERE concept_id = 5587 AND preferred_label = 'Germanic' AND normalized_label = 'germanic';
COMMIT;
