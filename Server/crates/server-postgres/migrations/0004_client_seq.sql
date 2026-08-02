-- Migration 4: Add client_seq and remove id/timestamp (now superseded by init migration)
-- This is a no-op when running on a fresh database with the final schema

-- Add client_seq column for deduplication (if not exists)
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name = 'library_events' AND column_name = 'client_seq') THEN
        ALTER TABLE library_events ADD COLUMN client_seq BIGINT NOT NULL DEFAULT 0;
    END IF;
END $$;

-- Only run cleanup on old schema that has id column
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.columns
               WHERE table_name = 'library_events' AND column_name = 'id') THEN
        -- Rename server_sequence to server_seq (if needed)
        IF EXISTS (SELECT 1 FROM information_schema.columns
                   WHERE table_name = 'library_events' AND column_name = 'server_sequence') THEN
            ALTER TABLE library_events RENAME COLUMN server_sequence TO server_seq;
        END IF;

        -- Drop the id column
        DROP INDEX IF EXISTS idx_event_id;
        ALTER TABLE library_events DROP COLUMN IF EXISTS id;

        -- Drop the timestamp columns
        ALTER TABLE library_events DROP COLUMN IF EXISTS timestamp;
        ALTER TABLE library_events DROP COLUMN IF EXISTS server_timestamp;

        -- Remove old id primary key constraint if exists
        ALTER TABLE library_events DROP CONSTRAINT IF EXISTS library_events_pkey;

        -- Add new primary key based on server_seq
        IF NOT EXISTS (SELECT 1 FROM pg_constraint WHERE conname = 'library_events_pkey') THEN
            ALTER TABLE library_events ADD PRIMARY KEY (server_seq);
        END IF;
    END IF;
END $$;

-- Create unique index for append event dedup (idempotent)
CREATE UNIQUE INDEX IF NOT EXISTS idx_library_events_dedup
ON library_events(library_id, client_id, client_seq)
WHERE event_type NOT IN ('reading_position_updated', 'book_opened');

-- Drop old timestamp-based indexes if exist
DROP INDEX IF EXISTS idx_library_events_library_ts;
DROP INDEX IF EXISTS idx_library_events_library_server_ts;

-- Update server_seq index name (idempotent)
DROP INDEX IF EXISTS idx_library_events_library_server_seq;
CREATE INDEX IF NOT EXISTS idx_library_events_library_seq
ON library_events(library_id, server_seq);
