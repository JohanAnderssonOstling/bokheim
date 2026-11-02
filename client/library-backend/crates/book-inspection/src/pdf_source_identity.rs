//! Optional PDF provenance. These values never replace hashing the full file.

use crate::MetadataResult;
use content_address::{BookId, ContentHash};
use lopdf::{Document, Object, dictionary};
use std::io::{Read, Seek};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfSourceIdentity {
    pub book_id: BookId,
    pub canonical_hash: Option<ContentHash>,
}

impl PdfSourceIdentity {
    pub fn read(reader: impl Read + Seek) -> MetadataResult<Option<Self>> {
        let document = crate::pdf_identity_document(reader)?;
        Ok(Self::from_document(&document))
    }

    pub fn from_document(document: &Document) -> Option<Self> {
        let source = document.catalog().ok()?.get(b"BokheimSourceIdentity").ok()?.as_dict().ok()?;
        if source.get(b"Version").ok()?.as_i64().ok()? != 1 {
            return None;
        }
        let book_id = std::str::from_utf8(source.get(b"BookId").ok()?.as_str().ok()?).ok()?.parse().ok()?;
        let canonical_hash = match source.get(b"CanonicalHash") {
            Ok(value) => Some(std::str::from_utf8(value.as_str().ok()?).ok()?.parse().ok()?),
            Err(_) => None,
        };
        Some(Self { book_id, canonical_hash })
    }

    /// Add provenance while an exporter is already modifying this local PDF.
    /// No checksum of the resulting file is embedded: that would be circular.
    pub fn write(self, document: &mut Document) -> MetadataResult<()> {
        let mut source = dictionary! { "Version" => 1, "BookId" => Object::string_literal(self.book_id.to_hex()) };
        if let Some(hash) = self.canonical_hash {
            source.set("CanonicalHash", Object::string_literal(hash.to_hex()));
        }
        document.catalog_mut()?.set("BokheimSourceIdentity", source);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_preserves_identity_without_claiming_the_resulting_byte_hash() {
        let mut document = Document::with_version("1.7");
        let pages = document.add_object(dictionary! { "Type" => "Pages", "Kids" => Vec::<Object>::new(), "Count" => 0 });
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        let mut original = Vec::new();
        document.save_to(&mut original).unwrap();
        let canonical = ContentHash::from(blake3::hash(&original));
        let source = PdfSourceIdentity { book_id: canonical.into(), canonical_hash: Some(canonical) };
        source.write(&mut document).unwrap();
        let mut exported = Vec::new();
        document.save_to(&mut exported).unwrap();
        assert_ne!(ContentHash::from(blake3::hash(&exported)), canonical);
        assert_eq!(PdfSourceIdentity::from_document(&Document::load_mem(&exported).unwrap()), Some(source));
        assert_eq!(PdfSourceIdentity::read(std::io::Cursor::new(exported)).unwrap(), Some(source));
    }
}
