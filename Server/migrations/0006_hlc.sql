ALTER TABLE library_events ADD COLUMN IF NOT EXISTS hlc_pt BIGINT;
ALTER TABLE library_events ADD COLUMN IF NOT EXISTS hlc_l BIGINT;

UPDATE library_events
SET hlc_pt = COALESCE(hlc_pt, timestamp),
    hlc_l = COALESCE(hlc_l, 0)
WHERE hlc_pt IS NULL OR hlc_l IS NULL;

ALTER TABLE library_events ALTER COLUMN hlc_pt SET NOT NULL;
ALTER TABLE library_events ALTER COLUMN hlc_l SET NOT NULL;

ALTER TABLE library_events DROP COLUMN IF EXISTS hlc_node;
