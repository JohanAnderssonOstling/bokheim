//! Durable diagnostics tied to an exact publication, rather than a whole pass.
use crate::query_methods::{blob_column, query_rows};
use crate::sync::{MutationIssuesSql, storage_i64};
use crate::sync_status::{SyncIssueDetail, SyncIssueSummary};
use crate::{Database, DatabaseError};
use sync_common::MutationIssue;

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

    pub fn sync_issue_details(&self) -> Result<Vec<SyncIssueDetail>, DatabaseError> {
        Ok(query_rows(
            |read| self.connection.sync_issue_details(read),
            |row| {
                let issue = sync_common::wire::decode(blob_column(row, 2)?, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| crate::sync::invalid_text_column(2, error.to_string()))?;
                Ok(SyncIssueDetail { kind: row.get(0)?, item_name: row.get(1)?, issue })
            },
        )?)
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn sync_record_mutation_issues(issues: &[MutationIssue]) -> ();
    arguments (Vec<MutationIssue>,) = (issues.to_vec(),);
    execute database, arguments => database.sync_record_mutation_issues(&arguments.0);
}
