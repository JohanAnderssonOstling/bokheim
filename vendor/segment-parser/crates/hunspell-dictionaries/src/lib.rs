use std::{fs, io::{self, Write}, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

const ASSET_VERSION: &str = "2026-10-08-v1";
static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

struct BundledDictionary {
    stem: &'static str,
    aff: &'static [u8],
    dic: &'static [u8],
}

impl BundledDictionary {
    const fn new(stem: &'static str, aff: &'static [u8], dic: &'static [u8]) -> Self { Self { stem, aff, dic } }
}

static SWEDISH: BundledDictionary = BundledDictionary::new("sv_SE", include_bytes!("../assets/sv_SE.aff"), include_bytes!("../assets/sv_SE.dic"));
static GERMAN: BundledDictionary = BundledDictionary::new("de_DE", include_bytes!("../assets/de_DE.aff"), include_bytes!("../assets/de_DE.dic"));
static ENGLISH: BundledDictionary = BundledDictionary::new("en_GB-large", include_bytes!("../assets/en_GB-large.aff"), include_bytes!("../assets/en_GB-large.dic"));

pub struct DictionaryPaths {
    aff: PathBuf,
    dic: PathBuf,
}

impl DictionaryPaths {
    fn new(aff: PathBuf, dic: PathBuf) -> Self { Self { aff, dic } }
    pub fn aff(&self) -> &Path { &self.aff }
    pub fn dic(&self) -> &Path { &self.dic }
    pub fn into_pair(self) -> (PathBuf, PathBuf) { (self.aff, self.dic) }
}

pub fn resolve(lang_tag: &str) -> io::Result<Option<DictionaryPaths>> {
    bundled_dictionary(lang_tag).map(extract).transpose()
}

fn bundled_dictionary(lang_tag: &str) -> Option<&'static BundledDictionary> {
    match lang_tag.trim().split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase().as_str() {
        "sv" | "swe" => Some(&SWEDISH),
        "de" | "deu" | "ger" => Some(&GERMAN),
        "en" | "eng" => Some(&ENGLISH),
        _ => None,
    }
}

fn extract(dictionary: &BundledDictionary) -> io::Result<DictionaryPaths> {
    let directory = std::env::temp_dir().join("rusttrados").join("hunspell").join(ASSET_VERSION);
    fs::create_dir_all(&directory)?;
    let aff = directory.join(format!("{}.aff", dictionary.stem));
    let dic = directory.join(format!("{}.dic", dictionary.stem));
    write_if_changed(&aff, dictionary.aff)?;
    write_if_changed(&dic, dictionary.dic)?;
    Ok(DictionaryPaths::new(aff, dic))
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == bytes) { return Ok(()); }
    let extension = path.extension().and_then(|extension| extension.to_str()).unwrap_or("asset");
    let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
    let staged = path.with_extension(format!("{extension}.{}.{}.tmp", std::process::id(), sequence));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&staged)?;
    file.write_all(bytes)?;
    file.flush()?;
    drop(file);
    match fs::rename(&staged, path) {
        Ok(()) => Ok(()),
        Err(_) if fs::read(path).is_ok_and(|existing| existing == bytes) => fs::remove_file(staged),
        Err(error) => {
            let _ = fs::remove_file(staged);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_every_bundled_language_on_every_platform() {
        ["sv", "sv-SE", "swe", "de", "de-DE", "deu", "ger", "en", "en-GB", "eng"].into_iter().for_each(|tag| {
            let paths = resolve(tag).unwrap().unwrap();
            assert!(paths.aff().is_file(), "missing aff for {tag}");
            assert!(paths.dic().is_file(), "missing dic for {tag}");
        });
    }

    #[test]
    fn rejects_languages_without_bundled_assets() { assert!(resolve("fr-FR").unwrap().is_none()); }
}
