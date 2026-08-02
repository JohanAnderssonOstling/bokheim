-- A mutation UUID, rather than an actor-local counter, is the retry identity.
-- Existing rows receive deterministic UUID-shaped identifiers derived from
-- their primary key so rolling upgrades preserve every accepted event.
ALTER TABLE library_events
    ADD COLUMN IF NOT EXISTS event_id TEXT;

UPDATE library_events
SET event_id = substr(md5('bokheim-event:' || server_seq::text), 1, 8) || '-' ||
               substr(md5('bokheim-event:' || server_seq::text), 9, 4) || '-' ||
               substr(md5('bokheim-event:' || server_seq::text), 13, 4) || '-' ||
               substr(md5('bokheim-event:' || server_seq::text), 17, 4) || '-' ||
               substr(md5('bokheim-event:' || server_seq::text), 21, 12)
WHERE event_id IS NULL;

ALTER TABLE library_events
    ALTER COLUMN event_id SET NOT NULL;

DROP INDEX IF EXISTS idx_library_events_dedup;

CREATE UNIQUE INDEX idx_library_events_event_id
    ON library_events(library_id, event_id);

DROP INDEX IF EXISTS idx_library_events_lww;

-- Re-evaluate retained singleton winners with the complete total version now
-- that event identity is available as the final cloned-replica tie-break.
WITH ranked AS (
    SELECT server_seq,
           row_number() OVER (
               PARTITION BY library_id, entity_id, event_type
               ORDER BY hlc_pt DESC, hlc_l DESC, version_rank DESC,
                        client_id COLLATE "C" DESC, client_seq DESC,
                        event_id COLLATE "C" DESC
           ) AS position
    FROM library_events
    WHERE event_type IN ('dir_state', 'reading_position_updated', 'book_opened', 'book_metadata_updated')
)
DELETE FROM library_events AS event
USING ranked
WHERE event.server_seq = ranked.server_seq AND ranked.position > 1;

-- Reissue every retained singleton after the old high-water mark. Existing
-- clients may already have passed its historical sequence and must observe the
-- winner selected with the new event-id tie-break.
SELECT setval(
    pg_get_serial_sequence('library_events', 'server_seq'),
    COALESCE(MAX(server_seq), 1),
    true
)
FROM library_events;

UPDATE library_events
SET server_seq = nextval(pg_get_serial_sequence('library_events', 'server_seq'))
WHERE event_type IN ('dir_state', 'reading_position_updated', 'book_opened', 'book_metadata_updated');

CREATE UNIQUE INDEX idx_library_events_lww
    ON library_events(library_id, entity_id, event_type)
    WHERE event_type IN ('dir_state', 'reading_position_updated', 'book_opened', 'book_metadata_updated');
