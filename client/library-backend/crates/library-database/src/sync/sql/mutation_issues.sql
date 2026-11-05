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

-- name: sync_issue_details?
SELECT i.state_kind,COALESCE(b.title,ab.title,d.name),i.issue FROM sync_mutation_issue i
LEFT JOIN book b ON b.content_hash=i.state_key
LEFT JOIN annotation a ON i.state_kind='annotation' AND a.id=CAST(i.state_key AS TEXT)
LEFT JOIN book ab ON ab.row_id=a.book_row_id
LEFT JOIN dir d ON i.state_kind IN ('directory_name','directory_parent','directory_lifecycle') AND d.id=CAST(i.state_key AS TEXT)
ORDER BY CASE i.issue_class WHEN 1 THEN 0 WHEN 2 THEN 1 ELSE 2 END,i.publisher_seq
LIMIT 10;
