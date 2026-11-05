//! Typed failures carried through io::Error without classifying message text.
use library_model::{AnnotationExportFailure, AnnotationExportFailureKind};
use std::{error::Error, fmt, io};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct ExportFailure {
    details: AnnotationExportFailure,
    message: String,
}

pub(crate) fn failure(kind: AnnotationExportFailureKind, message: impl fmt::Display) -> io::Error {
    io::Error::other(ExportFailure { details: AnnotationExportFailure { kind, file_written: false }, message: message.to_string() })
}
pub(crate) fn tracking(message: impl fmt::Display) -> io::Error {
    io::Error::other(ExportFailure { details: AnnotationExportFailure { kind: AnnotationExportFailureKind::Tracking, file_written: true }, message: message.to_string() })
}
pub(crate) fn failure_details(error: &io::Error) -> AnnotationExportFailure {
    if let Some(failure) = error.get_ref().and_then(|value| value.downcast_ref::<ExportFailure>()) {
        return failure.details;
    }
    let kind = match error.kind() {
        io::ErrorKind::WouldBlock => AnnotationExportFailureKind::Busy,
        io::ErrorKind::NotFound | io::ErrorKind::NotConnected => AnnotationExportFailureKind::Unavailable,
        io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => AnnotationExportFailureKind::InvalidData,
        _ => AnnotationExportFailureKind::Io,
    };
    AnnotationExportFailure { kind, file_written: false }
}

/// Parser/writer errors may wrap genuine filesystem failures. Preserve their
/// native retryability; malformed document data requires user attention.
pub(crate) fn input(error: impl Into<Box<dyn Error + Send + Sync>>) -> io::Error {
    let error = error.into();
    let mut cause: Option<&(dyn Error + 'static)> = Some(error.as_ref());
    while let Some(value) = cause {
        if let Some(io) = value.downcast_ref::<io::Error>() {
            if io.kind() != io::ErrorKind::Other {
                return io::Error::new(io.kind(), error.to_string());
            }
        }
        cause = value.source();
    }
    failure(AnnotationExportFailureKind::InvalidData, error)
}
