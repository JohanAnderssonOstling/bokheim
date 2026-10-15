-- name: sync_issue_record !
-- param: publisher_seq: i64
-- param: issue_class: i64
-- param: issue: &[u8]
INSERT INTO sync_mutation_issue(publisher_seq,state_kind,state_key,state_subkey,issue_class,issue)
SELECT replica_seq,state_kind,state_key,state_subkey,:issue_class,:issue FROM sync_outbox WHERE replica_seq=:publisher_seq
ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET publisher_seq=excluded.publisher_seq,issue_class=excluded.issue_class,issue=excluded.issue
WHERE publisher_seq<>excluded.publisher_seq OR issue_class<>excluded.issue_class OR issue IS NOT excluded.issue;

-- name: sync_issue_summary ->
SELECT COALESCE(SUM(issue_class=0),0), COALESCE(SUM(issue_class=1),0), COALESCE(SUM(issue_class=2),0) FROM sync_mutation_issue;

-- name: sync_issue_rows ?
SELECT publisher_seq,state_kind,state_key,state_subkey,issue FROM sync_mutation_issue ORDER BY publisher_seq;
