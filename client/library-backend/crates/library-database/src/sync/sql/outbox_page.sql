SELECT state_kind, state_key, body, changed_at, replica_seq,
       state_subkey, winning_version
FROM sync_outbox
WHERE (:lifecycles IS NULL OR (state_kind = 'book_lifecycle') = :lifecycles)
  AND body IS NOT NULL AND replica_seq > :after AND replica_seq <= :through
  AND NOT EXISTS (
      SELECT 1 FROM sync_mutation_issue issue
      WHERE issue.publisher_seq = sync_outbox.replica_seq AND issue.issue_class = 2
  )
ORDER BY replica_seq ASC
LIMIT :limit;
