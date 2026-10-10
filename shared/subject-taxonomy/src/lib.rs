//! LCC call numbers and the compiled unified-taxonomy matcher. Shared by
//! `subject-projection` and the build script that bundles its taxonomy.

pub mod lcc;
pub mod range_storage;
pub mod runtime_index;
pub mod selector_format;
pub mod taxonomy_reader;

pub fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
