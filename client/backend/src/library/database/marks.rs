//! Native library-handle wrappers for shared annotation and bookmark persistence.

use super::*;
use crate::app::BackendError;
#[cfg(feature = "pdf")]
use crate::library::database as db;
use crate::library::LibraryBackend;
#[cfg(feature = "pdf")]
use app_contract::library::BookFormat;
#[cfg(feature = "pdf")]
use app_contract::reader_state::{AnnotationAnchor, PdfAnnotationAnchor};
use app_contract::reader_state::{ReaderAnnotation, ReaderBookmark};
#[cfg(feature = "pdf")]
use std::collections::HashSet;

#[cfg(feature = "pdf")]
fn pdf_is_writable(path: &std::path::Path) -> Result<bool, BackendError> {
    let metadata = std::fs::metadata(path).map_err(BackendError::operation)?;
    if metadata.permissions().readonly() {
        return Ok(false);
    }
    match std::fs::OpenOptions::new().write(true).open(path) {
        Ok(_) => Ok(true),
        Err(error) if matches!(error.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem) => Ok(false),
        Err(error) => Err(BackendError::operation(error)),
    }
}

#[cfg(feature = "pdf")]
fn external_pdf_annotation(content_hash: &ContentHash, annotation: crate::library::pdf_annotations::ParsedPdfAnnotation) -> ReaderAnnotation {
    let mut fingerprint = blake3::Hasher::new();
    fingerprint.update(content_hash.as_str().as_bytes());
    fingerprint.update(&annotation.page_index.to_le_bytes());
    fingerprint.update(&annotation.annotation_index.to_le_bytes());
    fingerprint.update(annotation.name.as_deref().unwrap_or_default().as_bytes());
    fingerprint.update(annotation.style.storage().as_bytes());
    fingerprint.update(annotation.note.as_bytes());
    fingerprint.update(&annotation.opacity.to_le_bytes());
    for rect in &annotation.rects {
        fingerprint.update(&rect.left.to_le_bytes());
        fingerprint.update(&rect.top.to_le_bytes());
        fingerprint.update(&rect.width.to_le_bytes());
        fingerprint.update(&rect.height.to_le_bytes());
    }
    let id = format!("{}{}", app_contract::reader_state::EXTERNAL_PDF_ANNOTATION_ID_PREFIX, fingerprint.finalize().to_hex());
    ReaderAnnotation {
        id,
        content_hash: content_hash.clone(),
        anchor: AnnotationAnchor::pdf(PdfAnnotationAnchor::new(annotation.page_index, annotation.rects, None)),
        exact_text: annotation.author,
        style: annotation.style,
        color: annotation.color,
        note: annotation.note,
        created_at: 0,
        modified_at: 0,
    }
}

impl LibraryBackend {
    pub fn annotations(&self, content_hash: ContentHash) -> Result<Vec<ReaderAnnotation>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        let mut stored = load_annotations(&conn, content_hash.clone()).map_err(BackendError::operation)?;
        #[cfg(feature = "pdf")]
        if db::get_book_format(&conn, &content_hash).map_err(BackendError::operation)? == Some(BookFormat::Pdf) {
            if let Some(path) = self.local_book_path(content_hash.clone())? {
                let mut native = crate::library::pdf_annotations::parse(&path).map_err(BackendError::operation)?;
                let native_ids = native.iter().filter_map(|annotation| annotation.bokheim_id.clone()).collect::<HashSet<_>>();
                if pdf_is_writable(&path)? {
                    let missing = stored.iter().filter(|annotation| annotation.pdf_anchor().is_some() && !native_ids.contains(annotation.id.as_str())).cloned().collect::<Vec<_>>();
                    if !missing.is_empty() {
                        self.prepare_pdf_working_copy(&conn, &content_hash, &path)?;
                        for annotation in &missing {
                            crate::library::pdf_annotations::upsert(&path, annotation).map_err(BackendError::operation)?;
                        }
                        self.stamp_pdf_working_copy(&conn, &content_hash, &path)?;
                        native = crate::library::pdf_annotations::parse(&path).map_err(BackendError::operation)?;
                    }
                }
                let stored_ids = stored.iter().map(|annotation| annotation.id.clone()).collect::<HashSet<_>>();
                stored.extend(native.into_iter().filter(|annotation| annotation.bokheim_id.as_ref().is_none_or(|id| !stored_ids.contains(id))).map(|annotation| external_pdf_annotation(&content_hash, annotation)));
            }
        }
        Ok(stored)
    }

    pub fn upsert_annotation(&self, annotation: &ReaderAnnotation) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        #[cfg(feature = "pdf")]
        if annotation.pdf_anchor().is_some() && db::get_book_format(&conn, &annotation.content_hash).map_err(BackendError::operation)? == Some(BookFormat::Pdf) {
            if let Some(path) = self.local_book_path(annotation.content_hash.clone())? {
                if pdf_is_writable(&path)? {
                    self.prepare_pdf_working_copy(&conn, &annotation.content_hash, &path)?;
                    crate::library::pdf_annotations::upsert(&path, annotation).map_err(BackendError::operation)?;
                    self.stamp_pdf_working_copy(&conn, &annotation.content_hash, &path)?;
                }
            }
        }
        store_annotation(&conn, annotation).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_annotation(&self, annotation_id: &str, modified_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        #[cfg(feature = "pdf")]
        if let Some(annotation) = annotation_by_id(&conn, annotation_id).map_err(BackendError::operation)? {
            if annotation.pdf_anchor().is_some() && db::get_book_format(&conn, &annotation.content_hash).map_err(BackendError::operation)? == Some(BookFormat::Pdf) {
                if let Some(path) = self.local_book_path(annotation.content_hash.clone())? {
                    if pdf_is_writable(&path)? {
                        self.prepare_pdf_working_copy(&conn, &annotation.content_hash, &path)?;
                        crate::library::pdf_annotations::remove(&path, annotation_id).map_err(BackendError::operation)?;
                        self.stamp_pdf_working_copy(&conn, &annotation.content_hash, &path)?;
                    }
                }
            }
        }
        remove_annotation(&conn, annotation_id, modified_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    #[cfg(feature = "pdf")]
    fn prepare_pdf_working_copy(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, working_path: &std::path::Path) -> Result<(), BackendError> {
        let remote = db::get_remote_asset_hashes(conn, crate::BlobKind::Book).map_err(BackendError::operation)?.contains(content_hash);
        if !remote {
            let base = crate::library::pdf_annotations::stage_base(&self.library_root, working_path, content_hash).map_err(BackendError::operation)?;
            let base = base.to_str().ok_or_else(|| BackendError::operation("PDF base staging path is not valid UTF-8"))?;
            db::upsert_blob_local_ref(conn, crate::BlobKind::Book, content_hash, base, true).map_err(BackendError::operation)?;
        }
        Ok(())
    }

    #[cfg(feature = "pdf")]
    fn stamp_pdf_working_copy(&self, conn: &rusqlite::Connection, content_hash: &ContentHash, working_path: &std::path::Path) -> Result<(), BackendError> {
        for placement in db::get_book_paths_by_content_hash(conn, content_hash).map_err(BackendError::operation)? {
            if placement.rel_path.resolve_under(&self.library_root) == working_path {
                db::stamp_book_dir_local_hash_by_rel_path(conn, &placement.rel_path, content_hash.clone(), working_path).map_err(BackendError::operation)?;
            }
        }
        Ok(())
    }

    pub fn bookmarks(&self, content_hash: ContentHash) -> Result<Vec<ReaderBookmark>, BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        load_bookmarks(&conn, content_hash).map_err(BackendError::operation)
    }

    pub fn create_bookmark(&self, bookmark: &ReaderBookmark) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        insert_bookmark(&conn, bookmark).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }

    pub fn delete_bookmark(&self, bookmark_id: &str, deleted_at: i64) -> Result<(), BackendError> {
        let conn = self.db.get().map_err(BackendError::operation)?;
        remove_bookmark(&conn, bookmark_id, deleted_at).map_err(BackendError::operation)?;
        self.app()?.request_library_sync(self.id().clone(), false);
        Ok(())
    }
}
