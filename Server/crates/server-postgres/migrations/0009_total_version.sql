-- Complete conflict versions and repair the singleton/dedup predicates without
-- modifying migrations that may already have been applied.
ALTER TABLE library_events
    ADD COLUMN IF NOT EXISTS version_rank SMALLINT NOT NULL DEFAULT 0;

UPDATE library_events
SET version_rank = CASE
    WHEN event_type IN ('book_deleted', 'book_dir_removed', 'book_dir_deleted') THEN 1
    WHEN event_type = 'dir_state'
     AND COALESCE(data->>'deleted', 'false') IN ('true', '1') THEN 1
    ELSE 0
END;

-- `dir_state` used to be append-only, so an upgraded database may contain
-- several rows for one directory. Keep the same winner the new singleton
-- upsert would choose before creating its unique index.
WITH ranked_dir_state AS (
    SELECT
        server_seq,
        ROW_NUMBER() OVER (
            PARTITION BY library_id, entity_id, event_type
            ORDER BY hlc_pt DESC,
                     hlc_l DESC,
                     version_rank DESC,
                     client_id COLLATE "C" DESC,
                     client_seq DESC
        ) AS version_position
    FROM library_events
    WHERE event_type = 'dir_state'
)
DELETE FROM library_events AS event
USING ranked_dir_state AS ranked
WHERE event.server_seq = ranked.server_seq
  AND ranked.version_position > 1;

-- Give each retained directory value a fresh cursor position. A client whose
-- cursor had already passed the winning historical row must pull it again;
-- otherwise that client could retain a losing pre-migration value forever.
SELECT setval(
    pg_get_serial_sequence('library_events', 'server_seq'),
    COALESCE(MAX(server_seq), 1),
    true
)
FROM library_events;

UPDATE library_events
SET server_seq = nextval(pg_get_serial_sequence('library_events', 'server_seq'))
WHERE event_type = 'dir_state';

DROP INDEX IF EXISTS idx_library_events_lww;
DROP INDEX IF EXISTS idx_library_events_dedup;

CREATE UNIQUE INDEX idx_library_events_lww
    ON library_events(library_id, entity_id, event_type)
    WHERE event_type IN ('dir_state', 'reading_position_updated', 'book_opened');

CREATE UNIQUE INDEX idx_library_events_dedup
    ON library_events(library_id, client_id, client_seq)
    WHERE event_type NOT IN ('dir_state', 'reading_position_updated', 'book_opened');
