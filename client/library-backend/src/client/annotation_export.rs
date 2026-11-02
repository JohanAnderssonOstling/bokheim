use super::LibraryClient;
use crate::{BackendError, LibraryCommand, PendingCommand, ReadyCommand};
use library_model::{AnnotationExportFormat, AnnotationExportOptions, AnnotationExportPlan, AnnotationExportProgress, AnnotationExportReport};
use uuid::Uuid;

impl LibraryClient {
    pub async fn annotation_export_running_readers(&self, formats: Vec<AnnotationExportFormat>) -> Result<Vec<AnnotationExportFormat>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::AnnotationExportRunningReaders { formats })).await
    }

    pub async fn automatic_annotation_export(&self) -> Result<library_model::AutomaticAnnotationExportStatus, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::AutomaticAnnotationExport)).await
    }
    pub async fn set_automatic_annotation_export(&self, options: Option<AnnotationExportOptions>) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::SetAutomaticAnnotationExport { options })).await
    }
    pub async fn retry_automatic_annotation_export(&self) -> Result<(), BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::RetryAutomaticAnnotationExport)).await
    }
    pub async fn annotation_export_plan(&self, options: AnnotationExportOptions) -> Result<AnnotationExportPlan, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::AnnotationExportPlan { options })).await
    }
    pub async fn export_annotations(&self, export_id: Uuid, options: AnnotationExportOptions) -> Result<AnnotationExportReport, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::ExportAnnotations { export_id, options })).await
    }
    pub async fn annotation_export_progress(&self, export_id: Uuid) -> Result<Option<AnnotationExportProgress>, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::AnnotationExportProgress { export_id })).await
    }
    pub async fn cancel_annotation_export(&self, export_id: Uuid) -> Result<bool, BackendError> {
        self.request(LibraryCommand::Ready(ReadyCommand::CancelAnnotationExport { export_id })).await
    }
}
