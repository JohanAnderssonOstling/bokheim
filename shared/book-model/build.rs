use serde::Deserialize;
use std::{collections::HashSet, env, error::Error, fs, path::PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Format {
    variant: String,
    code: u8,
    kind: String,
    canonical_extension: String,
    browser_download: bool,
    android_view: bool,
    files: Vec<FileType>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileType {
    extensions: Vec<String>,
    mime_types: Vec<MediaType>,
    picker_extensions: Option<Vec<String>>,
    picker_mime_types: Option<Vec<String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MediaType {
    name: String,
    value: String,
    acquisition_priority: Option<u8>,
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=formats.json");
    let formats: Vec<Format> = serde_json::from_str(&fs::read_to_string("formats.json")?)?;
    let mut codes = HashSet::new();
    let mut variants = HashSet::new();
    let mut extensions = HashSet::new();
    let mut mime_names = HashSet::new();
    let mut mimes = HashSet::new();
    let mut priorities = HashSet::new();
    let mut picker_extensions = Vec::new();
    let mut rows = Vec::new();
    let mut constants = String::new();
    for format in &formats {
        if !format.code.is_power_of_two()
            || !codes.insert(format.code)
            || !variants.insert(&format.variant)
            || !format.variant.chars().all(|c| c.is_ascii_alphanumeric())
            || !matches!(format.kind.as_str(), "Text" | "Pdf" | "Audio" | "Comic")
        {
            return Err("invalid format identity, stored bit or reader kind".into());
        }
        if format.android_view && !format.browser_download {
            return Err("Android VIEW must name a downloadable book format".into());
        }
        let mut canonical = false;
        for file in &format.files {
            if file.extensions.is_empty() {
                return Err("a file type needs an extension".into());
            }
            for extension in &file.extensions {
                if extension.is_empty() || extension.starts_with('.') || extension.ends_with('.') || !extension.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.') || !extensions.insert(extension) {
                    return Err("invalid or duplicate book extension".into());
                }
                canonical |= extension == &format.canonical_extension;
            }
            for extension in file.picker_extensions.as_ref().unwrap_or(&file.extensions) {
                if !picker_extensions.contains(extension) {
                    picker_extensions.push(extension.clone());
                }
            }
            // Picker-only aliases are not accepted as downloaded book MIME types.
            if let Some(types) = &file.picker_mime_types {
                if types.iter().any(|kind| kind.is_empty() || kind.chars().any(char::is_whitespace)) {
                    return Err("invalid picker media type".into());
                }
            }
            let mut media_rows = Vec::new();
            for media in &file.mime_types {
                if !mime_names.insert(&media.name)
                    || !mimes.insert(&media.value)
                    || media.name.is_empty()
                    || !media.name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                    || !media.value.contains('/')
                    || media.value.chars().any(|c| c.is_whitespace() || c == ';')
                {
                    return Err("invalid or duplicate book media type".into());
                }
                if media.acquisition_priority.is_some_and(|priority| !priorities.insert(priority)) {
                    return Err("duplicate acquisition preference".into());
                }
                constants.push_str(&format!("pub const {}: &str = {:?};\n", media.name, media.value));
                media_rows.push(format!("MediaType {{ value: {}, acquisition_priority: {:?} }}", media.name, media.acquisition_priority));
            }
            rows.push(format!("FileType {{ format: BookFormat::{}, extensions: &{:?}, media_types: &[{}] }}", format.variant, file.extensions, media_rows.join(",")));
        }
        if !canonical {
            return Err("canonical book extension must name a file type".into());
        }
    }
    let variants = formats.iter().map(|f| format!("{} = {}", f.variant, f.code)).collect::<Vec<_>>().join(",");
    let names = formats.iter().map(|f| format!("Self::{}", f.variant)).collect::<Vec<_>>().join(",");
    let codes = formats.iter().map(|f| format!("{} => Some(Self::{}),", f.code, f.variant)).collect::<String>();
    let extensions = formats.iter().map(|f| format!("Self::{} => {:?},", f.variant, f.canonical_extension)).collect::<String>();
    let kinds = formats.iter().map(|f| format!("Self::{} => BookKind::{},", f.variant, f.kind)).collect::<String>();
    let browser = formats.iter().map(|f| format!("Self::{} => {},", f.variant, f.browser_download)).collect::<String>();
    let generated = format!(
        "/// Supported book formats. Each stored discriminant is one immutable format-mask bit.\n#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]\n#[repr(u8)]\npub enum BookFormat {{{variants}}}\n\
        {constants}\nconst FILE_TYPES: &[FileType] = &[{}];\n\
        impl BookFormat {{\n\
        pub const ALL: [Self; {}] = [{names}];\n\
        pub const IMPORT_EXTENSIONS: [&'static str; {}] = {:?};\n\
        pub const fn from_code(code: i64) -> Option<Self> {{ match code {{ {codes} _ => None }} }}\n\
        pub const fn canonical_extension(self) -> &'static str {{ match self {{ {extensions} }} }}\n\
        pub const fn kind(self) -> BookKind {{ match self {{ {kinds} }} }}\n\
        pub const fn browser_download(self) -> bool {{ match self {{ {browser} }} }}\n}}\n",
        rows.join(","),
        formats.len(),
        picker_extensions.len(),
        picker_extensions,
    );
    fs::write(PathBuf::from(env::var("OUT_DIR")?).join("book_formats.rs"), generated)?;
    Ok(())
}
