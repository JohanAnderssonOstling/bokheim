use std::fmt::Formatter;
use crate::UnixMillis;
use content_address::ContentHash;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStyle {
    Highlight,
    Underline,
    Squiggly,
    Strikethrough,
}

impl AnnotationStyle {
    pub const fn storage(&self) -> &'static str {
        match self {
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::Squiggly => "squiggly",
            Self::Strikethrough => "strikethrough",
        }
    }

    pub fn parse_storage(value: &str) -> Option<Self> {
        match value {
            "highlight" => Some(Self::Highlight),
            "underline" => Some(Self::Underline),
            "squiggly" => Some(Self::Squiggly),
            "strikethrough" => Some(Self::Strikethrough),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct PdfAnnotationRect {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl PdfAnnotationRect {
    pub const fn new(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self { left, top, width, height }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PdfAnnotationAnchor {
    pub page_index: u32,
    pub rects: Vec<PdfAnnotationRect>,
    pub fallback_cfi: Option<String>,
}

impl PdfAnnotationAnchor {
    pub fn new(page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String>) -> Self {
        Self { page_index, rects, fallback_cfi }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AnnotationAnchor {
    EpubCfi { cfi: String },
    Pdf { page_index: u32, rects: Vec<PdfAnnotationRect>, fallback_cfi: Option<String> },
}

// The canonical anchor defines both its tagged JSON and historical binary
// serialization directly. No parallel enum or conversion is involved.
impl Serialize for AnnotationAnchor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        match self {
            Self::EpubCfi { cfi } => {
                if !serializer.is_human_readable() {
                    return serializer.serialize_newtype_variant("AnnotationAnchor", 0, "EpubCfi", cfi);
                }
                let mut fields = serializer.serialize_struct("AnnotationAnchor", 2)?;
                fields.serialize_field("kind", "epub_cfi")?;
                fields.serialize_field("cfi", cfi)?;
                fields.end()
            }
            Self::Pdf { page_index, rects, fallback_cfi } => {
                if !serializer.is_human_readable() {
                    return serializer.serialize_newtype_variant("AnnotationAnchor", 1, "Pdf", &(page_index, rects, fallback_cfi));
                }
                let mut fields = serializer.serialize_struct("AnnotationAnchor", 4)?;
                fields.serialize_field("kind", "pdf")?;
                fields.serialize_field("page_index", page_index)?;
                fields.serialize_field("rects", rects)?;
                fields.serialize_field("fallback_cfi", fallback_cfi)?;
                fields.end()
            }
        }
    }
}
impl<'de> Deserialize<'de> for AnnotationAnchor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{EnumAccess, MapAccess, VariantAccess};
        struct Anchor;
        impl<'de> serde::de::Visitor<'de> for Anchor {
            type Value = AnnotationAnchor;
            fn expecting(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str("an annotation anchor")
            }
            fn visit_enum<A: EnumAccess<'de>>(self, access: A) -> Result<Self::Value, A::Error> {
                let (tag, value) = access.variant::<u32>()?;
                match tag {
                    0 => Ok(AnnotationAnchor::EpubCfi { cfi: value.newtype_variant()? }),
                    1 => {
                        let (page_index, rects, fallback_cfi) = value.newtype_variant()?;
                        Ok(AnnotationAnchor::Pdf { page_index, rects, fallback_cfi })
                    }
                    _ => Err(serde::de::Error::custom("invalid binary annotation anchor")),
                }
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let (mut kind, mut cfi, mut page_index, mut rects, mut fallback_cfi) = (None, None, None, None, None);
                while let Some(key) = access.next_key::<String>()? {
                    match key.as_str() {
                        "kind" => {
                            if kind.is_some() {
                                return Err(serde::de::Error::duplicate_field("kind"));
                            }
                            kind = Some(access.next_value::<String>()?);
                        }
                        "cfi" => {
                            if cfi.is_some() {
                                return Err(serde::de::Error::duplicate_field("cfi"));
                            }
                            cfi = Some(access.next_value()?);
                        }
                        "page_index" => {
                            if page_index.is_some() {
                                return Err(serde::de::Error::duplicate_field("page_index"));
                            }
                            page_index = Some(access.next_value()?);
                        }
                        "rects" => {
                            if rects.is_some() {
                                return Err(serde::de::Error::duplicate_field("rects"));
                            }
                            rects = Some(access.next_value()?);
                        }
                        "fallback_cfi" => {
                            if fallback_cfi.is_some() {
                                return Err(serde::de::Error::duplicate_field("fallback_cfi"));
                            }
                            fallback_cfi = Some(access.next_value()?);
                        }
                        _ => {
                            access.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                match kind.as_deref().ok_or_else(|| serde::de::Error::missing_field("kind"))? {
                    "epub_cfi" => Ok(AnnotationAnchor::EpubCfi { cfi: cfi.ok_or_else(|| serde::de::Error::missing_field("cfi"))? }),
                    "pdf" => Ok(AnnotationAnchor::Pdf {
                        page_index: page_index.ok_or_else(|| serde::de::Error::missing_field("page_index"))?,
                        rects: rects.ok_or_else(|| serde::de::Error::missing_field("rects"))?,
                        fallback_cfi: fallback_cfi.unwrap_or(None),
                    }),
                    other => Err(serde::de::Error::unknown_variant(other, &["epub_cfi", "pdf"])),
                }
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_map(Anchor)
        } else {
            deserializer.deserialize_enum("AnnotationAnchor", &["EpubCfi", "Pdf"], Anchor)
        }
    }
}

impl AnnotationAnchor {
    pub fn epub_cfi(cfi: impl Into<String>) -> Self {
        Self::EpubCfi { cfi: cfi.into() }
    }
    pub fn pdf(anchor: PdfAnnotationAnchor) -> Self {
        Self::Pdf { page_index: anchor.page_index, rects: anchor.rects, fallback_cfi: anchor.fallback_cfi }
    }
    pub fn epub_cfi_value(&self) -> Option<&str> {
        match self {
            Self::EpubCfi { cfi } => Some(cfi),
            Self::Pdf { fallback_cfi, .. } => fallback_cfi.as_deref(),
        }
    }
    pub fn pdf_anchor(&self) -> Option<PdfAnnotationAnchor> {
        match self {
            Self::EpubCfi { .. } => None,
            Self::Pdf { page_index, rects, fallback_cfi } => Some(PdfAnnotationAnchor::new(*page_index, rects.clone(), fallback_cfi.clone())),
        }
    }
}

/// Annotation content, shared by storage, readers, and synchronized state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnnotationDetail {
    pub anchor: AnnotationAnchor,
    pub exact_text: String,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
    #[serde(deserialize_with = "deserialize_created_at")]
    pub created_at: UnixMillis,
    #[serde(default)]
    pub toc_ordinal: Option<i64>,
    #[serde(default)]
    pub progress: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationState {
    pub content_hash: ContentHash,
    pub detail: AnnotationDetail,
    pub modified_at: UnixMillis,
    pub deleted: bool,
}
impl std::ops::Deref for AnnotationState {
    type Target = AnnotationDetail;
    fn deref(&self) -> &Self::Target {
        &self.detail
    }
}
impl std::ops::DerefMut for AnnotationState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.detail
    }
}

// Historical detail blobs used a signed creation timestamp and normalized
// negative values to zero when rebuilding state. Keep that read compatibility.
fn deserialize_created_at<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<UnixMillis, D::Error> {
    if !deserializer.is_human_readable() {
        return UnixMillis::deserialize(deserializer);
    }
    struct Timestamp;
    impl<'de> serde::de::Visitor<'de> for Timestamp {
        type Value = UnixMillis;
        fn expecting(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
            f.write_str("an annotation creation timestamp")
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
            Ok(value)
        }
        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
            Ok(value.max(0) as u64)
        }
    }
    deserializer.deserialize_any(Timestamp)
}

impl AnnotationDetail {
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}
impl AnnotationState {
    pub fn detail_json(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&self.detail)
    }
    pub fn from_detail(content_hash: ContentHash, detail: &[u8], modified_at: i64, deleted: bool) -> Result<Self, serde_json::Error> {
        Ok(Self { content_hash, detail: AnnotationDetail::decode(detail)?, modified_at: modified_at.max(0) as u64, deleted })
    }
    pub fn validate(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.created_at > i64::MAX as u64 || self.modified_at > i64::MAX as u64 {
            return Err("annotation timestamps exceed the database range".into());
        }
        if self.toc_ordinal.is_some_and(|ordinal| ordinal < 0) {
            return Err("annotation TOC ordinal must be non-negative".into());
        }
        if self.progress.is_some_and(|progress| !progress.is_finite() || !(0.0..=100.0).contains(&progress)) {
            return Err("annotation progress must be finite and between 0 and 100".into());
        }
        Ok(())
    }
}

// Preserve the historical flat network/JSON schema and positional register
// encoding while keeping the annotation's content defined in exactly one type.
impl Serialize for AnnotationState {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !serializer.is_human_readable() {
            return (&self.content_hash, &self.detail.anchor, &self.exact_text, &self.style, &self.color, &self.note, self.created_at, self.modified_at, self.deleted, self.toc_ordinal, self.progress).serialize(serializer);
        }
        #[derive(Serialize)]
        struct Fields<'a> {
            content_hash: &'a ContentHash,
            #[serde(flatten)]
            detail: &'a AnnotationDetail,
            modified_at: UnixMillis,
            deleted: bool,
        }
        Fields { content_hash: &self.content_hash, detail: &self.detail, modified_at: self.modified_at, deleted: self.deleted }.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for AnnotationState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if !deserializer.is_human_readable() {
            let (content_hash, anchor, exact_text, style, color, note, created_at, modified_at, deleted, toc_ordinal, progress): (
                ContentHash,
                AnnotationAnchor,
                String,
                AnnotationStyle,
                String,
                String,
                UnixMillis,
                UnixMillis,
                bool,
                Option<i64>,
                Option<f32>,
            ) = Deserialize::deserialize(deserializer)?;
            return Ok(Self { content_hash, detail: AnnotationDetail { anchor, exact_text, style, color, note, created_at, toc_ordinal, progress }, modified_at, deleted });
        }
        #[derive(Deserialize)]
        struct Fields {
            content_hash: ContentHash,
            #[serde(flatten)]
            detail: AnnotationDetail,
            modified_at: UnixMillis,
            #[serde(default)]
            deleted: bool,
        }
        let fields = Fields::deserialize(deserializer)?;
        Ok(Self { content_hash: fields.content_hash, detail: fields.detail, modified_at: fields.modified_at, deleted: fields.deleted })
    }
}

/// Position through the book on the shared 0–100 progress scale. Returns
/// `None` for empty totals or out-of-range parts instead of clamping silently.
pub fn annotation_progress(part: f64, total: f64) -> Option<f32> {
    if !part.is_finite() || !total.is_finite() || total <= 0.0 || part < 0.0 || part > total {
        return None;
    }
    Some((part / total * 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_rejects_degenerate_parts() {
        assert_eq!(annotation_progress(55.53, 100.0), Some(55.53));
        assert_eq!(annotation_progress(0.0, 100.0), Some(0.0));
        assert_eq!(annotation_progress(100.0, 100.0), Some(100.0));
        assert_eq!(annotation_progress(1.0, 0.0), None);
        assert_eq!(annotation_progress(-1.0, 100.0), None);
        assert_eq!(annotation_progress(101.0, 100.0), None);
        assert_eq!(annotation_progress(f64::NAN, 100.0), None);
    }

    #[test]
    fn detail_blob_round_trips_positioning() {
        let state = AnnotationState {
            content_hash: ContentHash::new(&"a".repeat(64)),
            detail: crate::AnnotationDetail {
                anchor: AnnotationAnchor::epub_cfi("epubcfi(/6/2)"),
                exact_text: "Text".to_owned(),
                style: AnnotationStyle::Underline,
                color: "#fff".to_owned(),
                note: "note".to_owned(),
                created_at: 7,
                toc_ordinal: Some(5),
                progress: Some(42.5),
            },
            modified_at: 9,
            deleted: false,
        };
        let decoded = AnnotationState::from_detail(state.content_hash.clone(), &state.detail_json().unwrap(), 9, false).unwrap();
        assert_eq!(decoded, state);
    }
}
