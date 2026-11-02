//! Native export orchestration, independent of menus and dialogs.
use super::{job_registry::JobGuard, LibrarySession};
use crate::BackendError;
use library_annotation_export::{export_prepared, plan, ExportControl};
use library_model::{AnnotationExportOptions, AnnotationExportPlan, AnnotationExportProgress, AnnotationExportReport};
use uuid::Uuid;

pub(crate) type ExportJobGuard = JobGuard<AnnotationExportProgress, ExportControl>;

impl LibrarySession {
    pub(crate) fn prepare_annotation_export(&self, id: Uuid) -> Result<ExportJobGuard, BackendError> {
        self.runtime.annotation_exports.prepare(id, AnnotationExportProgress { completed: 0, total: 0, current: None }, ExportControl::default(), "an annotation export with this request ID is already active")
    }
    pub(crate) fn annotation_export_plan(&self, options: AnnotationExportOptions) -> Result<AnnotationExportPlan, BackendError> {
        let books = self.db.annotation_export_snapshot()?;
        let options = self.annotation_export_options_for_books(&books, options)?;
        let mut plan = plan(self.id, &books, &options).map_err(BackendError::operation)?;
        let root = self.assets.native_library_root();
        for item in &mut plan.items {
            if item.format == library_model::AnnotationExportFormat::Pdf {
                item.path = root.join(item.path.to_string_lossy().trim_start_matches('/'));
            }
        }
        Ok(plan)
    }
    pub(crate) fn annotation_export_progress(&self, id: Uuid) -> Option<AnnotationExportProgress> {
        self.runtime.annotation_exports.progress(id)
    }
    pub(crate) fn cancel_annotation_export(&self, id: Uuid) -> bool {
        self.runtime.annotation_exports.cancel(id)
    }
    pub(crate) async fn export_annotations(&self, options: AnnotationExportOptions, job: ExportJobGuard) -> Result<AnnotationExportReport, BackendError> {
        let _export = self.runtime.annotation_export_lock.lock().await;
        let mut books = self.db.annotation_export_snapshot()?;
        let remember = options.conflict == library_model::AnnotationExportConflict::Replace && options.overrides.is_empty();
        let options = self.annotation_export_options_for_books(&books, options)?;
        let planned = plan(self.id, &books, &options).map_err(BackendError::operation)?;
        let destinations = planned.items.clone();
        let before_import = self.db.sync_outbox_watermark()?;
        let install_readers = options.install_readers;
        let report = export_prepared(&self.db, &self.assets, self.id, options, &mut books, planned, job.control.clone(), |progress| {
            job.set_progress(progress);
        })
        .await
        .map_err(BackendError::operation)?;
        // Reconciliation may have authored ordinary annotation registers, even
        // when one destination could not subsequently be published.
        if self.db.sync_outbox_watermark()? != before_import {
            self.events.sync.wake();
            self.events.publish_update(library_model::LibraryUpdate::Annotations);
            self.events.annotation_exports.notify_one();
        }
        if remember {
            if let Err(error) = self.remember_annotation_export(&books, &destinations, &report, install_readers) {
                self.annotation_export_tracking_failed(&error);
                log::warn!("could not remember manual annotation export; automatic export will retry: {error}");
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::{
        commands::{prepare_library_request, PreparedRequest},
        handle::concurrency_tests::fixture,
        LibraryCommand, PendingCommand, ReadyCommand,
    };
    use library_model::{AnnotationExportConflict, AnnotationExportFormat};

    #[tokio::test]
    async fn annotation_export_registers_cancellation_before_admission_and_releases_job_state() {
        let root = tempfile::tempdir().unwrap();
        let (mut actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let id = Uuid::new_v4();
        let options = AnnotationExportOptions { destination: root.path().with_extension("export"), formats: vec![AnnotationExportFormat::Markdown], install_readers: false, conflict: AnnotationExportConflict::KeepBoth, overrides: vec![] };
        let operation = prepare_library_request(&mut actor, LibraryCommand::Pending(PendingCommand::ExportAnnotations { export_id: id, options })).unwrap();
        assert!(actor.session.annotation_export_progress(id).is_some());
        assert!(actor.session.prepare_annotation_export(id).is_err());
        let reply = prepare_library_request(&mut actor, LibraryCommand::Ready(ReadyCommand::CancelAnnotationExport { export_id: id })).unwrap();
        let PreparedRequest::Ready(reply) = reply else { panic!("cancellation must run immediately") };
        assert!(reply.take::<bool>().unwrap());
        let PreparedRequest::Pending(operation) = operation else { panic!("export must be scheduled asynchronously") };
        let report = operation.future.await.unwrap().take::<AnnotationExportReport>().unwrap();
        assert!(report.cancelled);
        assert!(actor.session.annotation_export_progress(id).is_none());
    }
}
