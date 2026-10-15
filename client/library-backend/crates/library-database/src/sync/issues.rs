//! Durable diagnostics tied to an exact publication, rather than a whole pass.
use crate::sync::{MutationIssuesSql, storage_i64};
use crate::sync_status::{LibrarySyncIssue, SyncIssueSummary};
use crate::{Database, DatabaseError};
use sync_common::{MutationIssue, ReplicaSeq, StateAddress};

impl Database {
    /// Capture issues before acknowledging submissions. An obsolete response
    /// cannot attach an issue to a newer edit of the same cell.
    pub fn sync_record_mutation_issues(&self, issues: &[MutationIssue]) -> Result<(), DatabaseError> {
        if issues.is_empty() {
            return Ok(());
        }
        let prepared = issues
            .iter()
            .map(|issue| {
                let (sequence, class) = match issue {
                    MutationIssue::Retryable(rejection) => (rejection.publisher_seq, 0),
                    MutationIssue::Rejected(rejection) => (rejection.publisher_seq, 1),
                    MutationIssue::Untransmittable(sequence) => (*sequence, 2),
                };
                Ok((storage_i64(sequence.get(), "issue publication sequence")?, class, sync_common::wire::encode(issue).map_err(DatabaseError::operation)?))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;
        self.write(|tx| {
            for (sequence, class, bytes) in &prepared {
                tx.sync_issue_record(*sequence, *class, bytes)?;
            }
            Ok(())
        })
    }

    pub fn sync_issue_summary(&self) -> Result<SyncIssueSummary, DatabaseError> {
        let (retryable, rejected, untransmittable): (i64, i64, i64) = self.connection.sync_issue_summary(|row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(DatabaseError::operation)?;
        Ok(SyncIssueSummary {
            retryable: u64::try_from(retryable).map_err(DatabaseError::operation)?,
            rejected: u64::try_from(rejected).map_err(DatabaseError::operation)?,
            untransmittable: u64::try_from(untransmittable).map_err(DatabaseError::operation)?,
        })
    }

    /// Detailed diagnostics remain available even after a rejected row leaves
    /// the outbox; the settings snapshot only needs the small count summary.
    pub fn sync_mutation_issues(&self) -> Result<Vec<LibrarySyncIssue>, DatabaseError> {
        let mut issues = Vec::new();
        self.connection.sync_issue_rows(|row| {
            issues.push((row.get::<_, i64>(0)?, StateAddress { kind: row.get(1)?, entity_key: row.get(2)?, entity_subkey: row.get(3)? }, row.get::<_, Vec<u8>>(4)?));
            Ok(())
        })?;
        issues
            .into_iter()
            .map(|(sequence, address, bytes)| {
                let sequence = u64::try_from(sequence).map_err(DatabaseError::operation)?;
                Ok(LibrarySyncIssue {
                    publisher_seq: ReplicaSeq::new(sequence).map_err(DatabaseError::operation)?,
                    address,
                    issue: sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?,
                })
            })
            .collect()
    }
}
