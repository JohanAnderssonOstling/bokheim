use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[path = "tag_model/view.rs"]
mod view;
pub use view::{SegmentTag, SegmentTags};

#[derive(Clone, Debug, Default)]
pub(super) struct TagNumbering<'a> {
    by_id: HashMap<&'a str, (usize, usize)>,
}

impl<'a> TagNumbering<'a> {
    pub(super) fn next(&mut self, id: &'a str) -> (usize, usize) {
        let next_local_id = self.by_id.len() + 1;
        let entry = self.by_id.entry(id).or_insert((next_local_id, 0));
        let numbered = *entry;
        entry.1 += 1;
        numbered
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TagKind {
    Open,
    Close,
    Standalone,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FormattingKind {
    Bold,
    Italic,
    Underline,
    Superscript,
    Subscript,
}

impl FormattingKind {
    pub const ALL: [Self; 5] = [Self::Bold, Self::Italic, Self::Underline, Self::Superscript, Self::Subscript];
    pub fn label(self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::Superscript => "superscript",
            Self::Subscript => "subscript",
        }
    }
    pub(super) fn packed_shift(self) -> u32 { u32::from(self as u8) * 6 }
}

pub(super) fn pack_formatting_counts(kinds: impl IntoIterator<Item = FormattingKind>) -> u32 {
    kinds.into_iter().fold(0u32, |counts, kind| {
        let shift = kind.packed_shift();
        let count = ((counts >> shift) & 0x3f).saturating_add(1).min(0x3f);
        (counts & !(0x3f << shift)) | (count << shift)
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagMetadataProperty {
    key: String,
    value: String,
}

impl TagMetadataProperty {
    pub fn new(key: String, value: String) -> Self { Self { key, value } }
    pub fn key(&self) -> &str { &self.key }
    pub fn value(&self) -> &str { &self.value }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagMetadata {
    definition_kinds: Vec<String>,
    tag_type: Option<String>,
    equiv_text: Option<String>,
    display_text: Option<String>,
    has_display_text: bool,
    original_name: Option<String>,
    original_id: Option<String>,
    original_ctype: Option<String>,
    target: Option<String>,
    keep_as_reference: bool,
    reference_content: Option<String>,
    has_non_translatable_type_section: Option<bool>,
    field_type: Option<String>,
    sdl_emoji: bool,
    formatting: Vec<FormattingKind>,
    properties: Vec<TagMetadataProperty>,
}

impl TagMetadata {
    pub fn definition_kinds(&self) -> &[String] { &self.definition_kinds }
    pub fn tag_type(&self) -> Option<&str> { self.tag_type.as_deref() }
    pub fn equiv_text(&self) -> Option<&str> { self.equiv_text.as_deref() }
    pub fn display_text(&self) -> Option<&str> { self.display_text.as_deref() }
    pub fn has_display_text(&self) -> bool { self.has_display_text }
    pub fn original_name(&self) -> Option<&str> { self.original_name.as_deref() }
    pub fn original_id(&self) -> Option<&str> { self.original_id.as_deref() }
    pub fn original_ctype(&self) -> Option<&str> { self.original_ctype.as_deref() }
    pub fn target(&self) -> Option<&str> { self.target.as_deref() }
    pub fn keep_as_reference(&self) -> bool { self.keep_as_reference }
    pub fn reference_content(&self) -> Option<&str> { self.reference_content.as_deref() }
    pub fn has_non_translatable_type_section(&self) -> Option<bool> { self.has_non_translatable_type_section }
    pub fn field_type(&self) -> Option<&str> { self.field_type.as_deref() }
    pub fn sdl_emoji(&self) -> bool { self.sdl_emoji }
    pub fn formatting(&self) -> &[FormattingKind] { &self.formatting }
    pub fn properties(&self) -> &[TagMetadataProperty] { &self.properties }
    pub fn push_definition_kind(&mut self, kind: String) { if !self.definition_kinds.contains(&kind) { self.definition_kinds.push(kind); } }
    pub fn set_original_name_if_empty(&mut self, value: Option<String>) { if self.original_name.is_none() { self.original_name = value; } }
    pub fn set_equiv_text_if_empty(&mut self, value: Option<String>) { if self.equiv_text.is_none() { self.equiv_text = value; } }
    pub fn push_property(&mut self, property: TagMetadataProperty) { self.properties.push(property); }
    pub fn set_tag_type(&mut self, value: impl Into<String>) { self.tag_type = Some(value.into()); }
    pub fn set_display_text(&mut self, value: impl Into<String>) { self.display_text = Some(value.into()); }
    pub fn set_has_display_text(&mut self, value: bool) { self.has_display_text = value; }
    pub fn set_original_id(&mut self, value: impl Into<String>) { self.original_id = Some(value.into()); }
    pub fn set_original_ctype(&mut self, value: impl Into<String>) { self.original_ctype = Some(value.into()); }
    pub fn set_target(&mut self, value: impl Into<String>) { self.target = Some(value.into()); }
    pub fn set_keep_as_reference(&mut self, value: bool) { self.keep_as_reference = value; }
    pub fn set_reference_content(&mut self, value: impl Into<String>) { self.reference_content = Some(value.into()); }
    pub fn set_has_non_translatable_type_section(&mut self, value: Option<bool>) { self.has_non_translatable_type_section = value; }
    pub fn set_field_type(&mut self, value: impl Into<String>) { self.field_type = Some(value.into()); }
    pub fn set_sdl_emoji(&mut self, value: bool) { self.sdl_emoji = value; }
    pub fn push_formatting(&mut self, kind: FormattingKind) { if !self.formatting.contains(&kind) { self.formatting.push(kind); } }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagData {
    id: String,
    name: String,
    kind: TagKind,
    raw: String,
    equiv_text: Option<String>,
    metadata: Option<Arc<TagMetadata>>,
}

impl TagData {
    pub fn new(id: String, name: String, kind: TagKind, raw: String, equiv_text: Option<String>, metadata: Option<Arc<TagMetadata>>) -> Self { Self { id, name, kind, raw, equiv_text, metadata } }
    pub fn id(&self) -> &str { &self.id }
    pub fn name(&self) -> &str { &self.name }
    pub fn kind(&self) -> TagKind { self.kind }
    pub fn raw(&self) -> &str { &self.raw }
    pub fn equiv_text(&self) -> Option<&str> { self.equiv_text.as_deref() }
    pub fn metadata(&self) -> Option<&Arc<TagMetadata>> { self.metadata.as_ref() }
    pub fn display_label(&self, local: usize) -> String {
        let equiv_text = self.equiv_text().map(clean_equiv_text).filter(|text| !text.is_empty());
        match self.kind() {
            TagKind::Standalone => equiv_text.map_or_else(|| format!("<{local}/>"), |text| format!("<{local}:{text}/>")),
            TagKind::Open => equiv_text.map_or_else(|| format!("<{local}>"), |text| format!("<{local}:{text}>")),
            TagKind::Close => equiv_text.map_or_else(|| format!("</{local}>"), |text| format!("</{local}:{text}>")),
        }
    }
    pub fn into_parts(self) -> (String, String, TagKind, String, Option<String>, Option<Arc<TagMetadata>>) { (self.id, self.name, self.kind, self.raw, self.equiv_text, self.metadata) }
}

fn clean_equiv_text(text: &str) -> &str { text.trim_matches(|character: char| matches!(character, '<' | '>' | '[' | ']')).trim_matches('/') }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagAnchor {
    offset: usize,
    tag: TagData,
}

impl TagAnchor {
    pub fn new(offset: usize, tag: TagData) -> Self { Self { offset, tag } }
    pub fn offset(&self) -> usize { self.offset }
    pub fn tag(&self) -> &TagData { &self.tag }
    pub fn into_parts(self) -> (usize, TagData) { (self.offset, self.tag) }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagScope {
    tag_ids: Box<[String]>,
}

impl TagScope {
    pub fn new(tag_ids: Vec<String>) -> Self { Self { tag_ids: tag_ids.into_boxed_slice() } }
    pub fn tag_ids(&self) -> &[String] { &self.tag_ids }
    pub fn is_empty(&self) -> bool { self.tag_ids.is_empty() }
}
