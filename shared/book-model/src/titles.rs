//! Title normalization shared by inspection and library ingestion.
use crate::BookDetails;

impl BookDetails {
    pub fn subtitle(&self) -> Option<&str> {
        self.subtitle.as_deref()
    }

    pub fn set_subtitle(&mut self, subtitle: &str) {
        let subtitle = subtitle.trim();
        self.subtitle = (!subtitle.is_empty()).then(|| subtitle.to_owned());
    }

    /// A known subtitle wins: strip a title suffix that already restates it.
    /// Otherwise split once on the first colon, retaining further colons in
    /// the subtitle.
    pub fn normalize_title(&mut self) {
        let original = self.title.trim().to_owned();
        if let Some(subtitle) = self.subtitle.as_deref() {
            if let Some(prefix) = original.strip_suffix(subtitle).and_then(|prefix| prefix.trim_end().strip_suffix(':')) {
                if !prefix.trim().is_empty() {
                    self.title = prefix.trim().to_owned();
                }
            }
            return;
        }
        let Some((main, tail)) = original.split_once(':') else { return };
        let (main, tail) = (main.trim(), tail.trim());
        if main.is_empty() || tail.is_empty() {
            return;
        }
        self.subtitle = Some(tail.to_owned());
        self.title = main.to_owned();
    }
}
