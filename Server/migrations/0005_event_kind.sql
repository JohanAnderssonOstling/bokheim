-- event_kind is no longer used; drop it if present.
ALTER TABLE library_events DROP COLUMN IF EXISTS event_kind;
