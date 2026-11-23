//! Client-owned typed protobuf sync-state format, independent of HTTP.
//!
//! Version 34 replaces the generic Serde value tree. Legacy decoding belongs
//! exclusively to library-cloud-upgrade; normal replicas accept only this version.
use crate::{BookAddition, BookFacts, BookLifecycleState, DirectoryLifecycleState, EnrichmentResult, LibraryStateEntry, ReadingPositionState};
use book_model::{
    record_transport, Annotation, AnnotationAnchor, AnnotationStyle, AudiobookChapter, AudiobookTrack, BookFormat, BookTocDocument, BookTocEntry, ChapterReadStatus, PdfAnnotationAnchor, PdfAnnotationRect, ReadingPosition, RecurringScope,
};
use content_address::{BookId, ContentHash};
use metadata_contract::audible::{LookupRequest, RecordingEvidence};
use pdf_view_common::{PdfReaderMetadata, PdfStoredAnnotation, PdfStoredAnnotationKind, PdfStoredPage};
use prost::Message;
use sync_common::{
    http::{self as typed, invalid, limit_exceeded, required, Field, Kind, Schema, WireError},
    DirId,
};

use typed::encoding::Encoder;

mod generated;
mod page_columns;
use page_columns::PageColumnsProto;

// Version 34 is unreleased; compact page columns are part of its initial schema.
pub const VERSION: u32 = sync_common::STATE_STORAGE_VERSION;
const MAX_BYTES: usize = sync_common::MAX_PUSH_BATCH_BYTES;

trait Codec: Sized {
    type Proto: Message + Default + Schema;
    fn encode_fields(&self, encoder: &mut Encoder<'_>) -> Result<(), WireError>;
    fn bounded_len(&self, limit: usize) -> Result<usize, WireError> {
        // Standalone navigation sizing shares the stored-state budgets. The full
        // state pass also accounts for its enclosing messages and other fields.
        type Root = <LibraryStateEntry as Codec>::Proto;
        let mut encoder = Encoder::with_limits(Self::Proto::fields(), Root::MAX_DEPTH, Root::MAX_FIELDS, limit)?;
        self.encode_fields(&mut encoder)?;
        Ok(encoder.len())
    }
    fn from_proto(value: Self::Proto) -> Result<Self, WireError>;
}

pub fn encode(value: &LibraryStateEntry) -> Result<Vec<u8>, WireError> {
    // Bound recursion and traversal before domain validation or buffer allocation.
    let length = value.bounded_len(MAX_BYTES - 16)?;
    let payload_limit = value.address().payload_byte_limit();
    if length + 16 > payload_limit {
        return Err(limit_exceeded(format_args!("state exceeds its sync delivery byte limit: maximum {payload_limit}")));
    }
    value.validate()?;
    let mut output = Vec::with_capacity(length + 16);
    let mut encoder = Encoder::new(&mut output);
    encoder.varint(1, VERSION.into())?;
    encoder.message(2, |encoder| value.encode_fields(encoder))?;
    // Validate the payload without constructing a second owned representation.
    // The two envelope fields have known lengths.
    let payload_offset = output.len() - length;
    typed::validate_message::<<LibraryStateEntry as Codec>::Proto>(&output[payload_offset..], MAX_BYTES)?;
    Ok(output)
}

pub fn decode(bytes: &[u8]) -> Result<LibraryStateEntry, WireError> {
    let message = typed::decode_message(bytes, MAX_BYTES, VERSION)?;
    let value = LibraryStateEntry::from_proto(message)?;
    value.validate()?;
    Ok(value)
}

pub(crate) fn navigation_encoded_len(value: &BookTocDocument) -> Result<usize, WireError> {
    value.bounded_len(512 * 1024)
}

pub(crate) fn toc_encoded_len(value: &Vec<BookTocEntry>) -> Result<usize, WireError> {
    value.bounded_len(512 * 1024)
}
