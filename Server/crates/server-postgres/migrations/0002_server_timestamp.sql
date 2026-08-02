-- Migration 2: Add server_timestamp (now superseded by init migration)
-- This is a no-op when running on a fresh database with the final schema
DO $$ BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.columns
               WHERE table_name = 'library_events' AND column_name = 'id') THEN
        -- Only run on old schema
        ALTER TABLE library_events
            ADD COLUMN IF NOT EXISTS server_timestamp bigint NOT NULL DEFAULT (floor(extract(epoch from now()) * 1000))::bigint;
    END IF;
END $$;
