-- Run against a freshly initialized schema. All fixture state is rolled back.
BEGIN;
INSERT INTO users(id) VALUES('immutable-record-test');
INSERT INTO libraries(id,user_id,name) VALUES('immutable-record-test','immutable-record-test','Test');
DO $$
DECLARE
    record_kind text;
    accepted_revision bigint;
    repaired_revision bigint;
BEGIN
    FOREACH record_kind IN ARRAY ARRAY['book_add','reading_position'] LOOP
        INSERT INTO sync_state(library_id,kind,entity_key,value,changed_at,replica_id,replica_seq,version_rank)
        VALUES('immutable-record-test',record_kind,decode(repeat('01',32),'hex'),decode('01','hex'),1,'first',1,0);
        SELECT server_seq INTO accepted_revision FROM sync_state
        WHERE library_id='immutable-record-test' AND kind=record_kind;
        INSERT INTO sync_state(library_id,kind,entity_key,value,changed_at,replica_id,replica_seq,version_rank)
        VALUES('immutable-record-test',record_kind,decode(repeat('01',32),'hex'),decode('02','hex'),2,'second',1,0)
        ON CONFLICT(library_id,kind,entity_key,entity_subkey) DO UPDATE SET
            value=EXCLUDED.value,changed_at=EXCLUDED.changed_at,replica_id=EXCLUDED.replica_id,
            replica_seq=EXCLUDED.replica_seq,server_seq=DEFAULT;
        IF record_kind='reading_position' THEN
            IF NOT EXISTS(SELECT 1 FROM sync_state WHERE library_id='immutable-record-test' AND kind=record_kind AND value=decode('02','hex')) THEN
                RAISE EXCEPTION 'mutable reading position did not advance';
            END IF;
        ELSE
            IF NOT EXISTS(SELECT 1 FROM sync_state WHERE library_id='immutable-record-test' AND kind=record_kind AND value=decode('01','hex') AND changed_at=1 AND replica_id='first') THEN
                RAISE EXCEPTION 'immutable record was replaced: %',record_kind;
            END IF;
            SELECT server_seq INTO repaired_revision FROM sync_state
            WHERE library_id='immutable-record-test' AND kind=record_kind;
            IF repaired_revision<=accepted_revision THEN
                RAISE EXCEPTION 'accepted record was not redelivered to its competing author';
            END IF;
            UPDATE sync_state SET value=decode('01','hex'),changed_at=1,replica_id='first',replica_seq=1,server_seq=DEFAULT
            WHERE library_id='immutable-record-test' AND kind=record_kind;
            IF NOT EXISTS(SELECT 1 FROM sync_state WHERE library_id='immutable-record-test' AND kind=record_kind AND server_seq=repaired_revision) THEN
                RAISE EXCEPTION 'exact retry churned the immutable record revision';
            END IF;
        END IF;
    END LOOP;
END $$;
ROLLBACK;
