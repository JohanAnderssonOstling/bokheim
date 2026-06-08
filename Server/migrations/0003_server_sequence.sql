-- Migration 3: Add server_sequence (now superseded by init migration)
-- This is a no-op when running on a fresh database with the final schema
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.columns
               WHERE table_name = 'library_events' AND column_name = 'id') THEN
        -- Only run on old schema - add server_sequence column
        IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                       WHERE table_name = 'library_events' AND column_name = 'server_sequence') THEN
            ALTER TABLE library_events ADD COLUMN server_sequence BIGSERIAL;

            -- Backfill existing events preserving order based on server_timestamp
            WITH ordered AS (
                SELECT id, ROW_NUMBER() OVER (ORDER BY server_timestamp, id) as new_seq
                FROM library_events
            )
            UPDATE library_events le
            SET server_sequence = o.new_seq
            FROM ordered o
            WHERE le.id = o.id;
        END IF;
    END IF;
END $$;

-- Create index (idempotent - will be created by init migration for fresh DBs)
CREATE INDEX IF NOT EXISTS idx_library_events_library_server_seq
ON library_events(library_id, server_seq);
