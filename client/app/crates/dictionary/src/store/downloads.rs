//! Explicit curated downloads and metadata-only recommendations.
use super::{DictionaryFile, DictionaryInfo, DictionaryStore, Storage};
use crate::DictionaryDownloads;
use crate::Result;
use dictionary_catalog::{dictionaries, find, for_language};
use std::collections::BTreeSet;
use std::sync::PoisonError;

#[derive(Default)]
pub(super) struct Activity {
    pub active: Option<String>,
    pub error: Option<String>,
}

/// Resolve only the languages supplied by the caller, preserving priority.
fn recommended(languages: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    languages.iter().filter_map(|tag| for_language(tag)).filter(|spec| seen.insert(spec.id.clone())).map(|spec| spec.id.clone()).collect()
}

struct ActiveDownload<'a, S>(&'a DictionaryStore<S>);
impl<S> Drop for ActiveDownload<'_, S> {
    fn drop(&mut self) {
        self.0.activity.lock().unwrap_or_else(PoisonError::into_inner).active = None;
        self.0.wake.send_replace(());
    }
}

impl<S: Storage> DictionaryStore<S> {
    pub async fn downloads(&self, languages: &[String]) -> Result<DictionaryDownloads> {
        let activity = self.activity.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(DictionaryDownloads { available: dictionaries().to_vec(), recommended: recommended(languages), active: activity.active.clone(), error: activity.error.clone() })
    }

    /// Called only for an explicit download request from Settings.
    pub async fn download(&self, id: String) -> Result<Vec<DictionaryInfo>> {
        let spec = find(&id).ok_or_else(|| String::from("This dictionary is not in the curated catalog"))?;
        let _download = self.download_lock.lock().await;
        {
            let _guard = self.storage.lock().await;
            let installed = self.catalog().await?;
            if installed.iter().any(|item| item.curated_id.as_deref() == Some(&id)) {
                return Ok(installed);
            }
        }
        {
            let mut activity = self.activity.lock().unwrap_or_else(PoisonError::into_inner);
            activity.active = Some(id.clone());
            activity.error = None;
        }
        let _active = ActiveDownload(self);
        self.wake.send_replace(());
        let url = spec.download_url();
        let result = async {
            let bytes = dictionary_catalog::download(spec, &url).await?;
            self.install(vec![DictionaryFile { name: format!("{id}.zip"), bytes }], Some(spec)).await
        }
        .await;
        if let Err(error) = &result {
            self.activity.lock().unwrap_or_else(PoisonError::into_inner).error = Some(format!("{}: {error}", spec.name));
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recommendations_normalize_and_deduplicate_supplied_languages() {
        let ids = recommended(&["sv-SE".into(), "en_US.UTF-8".into(), "deu".into(), "de-DE".into(), "fra".into(), "und".into()]);
        let languages: BTreeSet<_> = ids.iter().map(|id| find(id).unwrap().language.as_str()).collect();
        assert_eq!(languages, BTreeSet::from(["de", "en", "fr", "sv"]));
        assert_eq!(find(&ids[0]).unwrap().language, "sv", "the caller controls language priority");
    }
}
