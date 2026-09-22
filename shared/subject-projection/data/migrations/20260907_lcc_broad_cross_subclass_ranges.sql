-- Reviewed broad spans: D through DX (history), R through RZ (medicine).
-- Narrower existing ranges and descendant subjects retain precedence.
BEGIN IMMEDIATE;
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('D1','DX301',3319);
INSERT INTO lcc_range(start_code,end_code,concept_id) VALUES('R','RZ',2725);
UPDATE taxonomy_meta SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='release_id';
COMMIT;
