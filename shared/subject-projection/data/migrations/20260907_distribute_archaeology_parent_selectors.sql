BEGIN;

-- The six children below partition CC1–CC960 completely, so the nonleaf
-- Archaeology umbrella must not retain any permissive legacy ranges.
DELETE FROM lcc_range WHERE concept_id=3484;

-- Remove the accidental umbrella range that overlaps every specialist child.
-- Archaeology Reference retains its proper CC1–CC71.9 range and exact CC code.
DELETE FROM lcc_range
WHERE concept_id=12945 AND class_letters='CC'
  AND start_value='1' AND end_value='960';

-- Exact selectors on Archaeology were overriding the already-curated child
-- ranges. Move each into the one existing child whose LCC interval owns it.
UPDATE lcc_selector SET concept_id=12947
WHERE concept_id=3484 AND selector IN ('CC107');

UPDATE lcc_selector SET concept_id=12948
WHERE concept_id=3484 AND selector IN ('CC135','CC136','CC138','CC140','CC176');

UPDATE lcc_selector SET concept_id=12949
WHERE concept_id=3484 AND selector IN ('CC207','CC253','CC260');

UPDATE lcc_selector SET concept_id=12950
WHERE concept_id=3484 AND selector IN ('CC600','CC700','CC710','CC960');

UPDATE lcc_selector SET concept_id=12946
WHERE concept_id=3484 AND selector IN (
 'CC72','CC72.4','CC72.7','CC76.4','CC77.5','CC78.3','CC78.6','CC78.7',
 'CC78.8','CC79.3','CC80.4','CC80.6','CC81','CC81.5','CC82.3','CC82.6',
 'CC82.8','CC83');

COMMIT;
