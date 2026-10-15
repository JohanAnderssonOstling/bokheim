CREATE TABLE IF NOT EXISTS sync_mutation_issue (
    publisher_seq INTEGER PRIMARY KEY CHECK(publisher_seq > 0),
    state_kind TEXT NOT NULL,
    state_key BLOB NOT NULL,
    state_subkey TEXT NOT NULL,
    issue_class INTEGER NOT NULL CHECK(issue_class IN (0, 1, 2)),
    issue BLOB NOT NULL,
    UNIQUE(state_kind, state_key, state_subkey)
);

-- A new queue generation supersedes any issue for the previous publication.
CREATE TRIGGER IF NOT EXISTS sync_issue_new_publication AFTER INSERT ON sync_outbox
BEGIN
    DELETE FROM sync_mutation_issue WHERE state_kind=NEW.state_kind AND state_key=NEW.state_key AND state_subkey=NEW.state_subkey AND publisher_seq<>NEW.replica_seq;
END;
CREATE TRIGGER IF NOT EXISTS sync_issue_replaced_publication AFTER UPDATE OF replica_seq ON sync_outbox
BEGIN
    DELETE FROM sync_mutation_issue WHERE state_kind=NEW.state_kind AND state_key=NEW.state_key AND state_subkey=NEW.state_subkey AND publisher_seq<>NEW.replica_seq;
END;
-- Permanent rejection details survive acknowledgment. A later acceptance or
-- removal clears retryable/oversized issues for that exact queue generation.
CREATE TRIGGER IF NOT EXISTS sync_issue_completed_publication AFTER DELETE ON sync_outbox
BEGIN
    DELETE FROM sync_mutation_issue WHERE publisher_seq=OLD.replica_seq AND issue_class<>1;
END;
