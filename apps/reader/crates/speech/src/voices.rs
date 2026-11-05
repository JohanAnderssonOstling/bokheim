//! Device voice identities and language matching shared by every speech adapter.
use serde::{Deserialize, Serialize};

/// An installed voice whose platform provider explicitly reports local synthesis.
/// Adapters must revalidate this property before each synthesis request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceVoice {
    pub id: String,
    pub name: String,
    pub language: String,
}

pub fn language_base(language: &str) -> String {
    let base = language.trim().split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
    match base.as_str() {
        "eng" => "en",
        "swe" => "sv",
        "deu" | "ger" => "de",
        "fra" | "fre" => "fr",
        "spa" => "es",
        "por" => "pt",
        "ita" => "it",
        "nld" | "dut" => "nl",
        "dan" => "da",
        "nor" => "no",
        "fin" => "fi",
        "pol" => "pl",
        "rus" => "ru",
        "ukr" => "uk",
        "ara" => "ar",
        "zho" | "chi" => "zh",
        "jpn" => "ja",
        "kor" => "ko",
        "und" | "mul" | "zxx" => "",
        _ => return base,
    }
    .into()
}

pub fn voice_matches_languages(voice: &DeviceVoice, languages: &[String]) -> bool {
    let declared: Vec<_> = languages.iter().map(|language| language_base(language)).filter(|language| !language.is_empty()).collect();
    declared.is_empty() || declared.contains(&language_base(&voice.language))
}

pub fn matching_device_voices(voices: &[DeviceVoice], languages: &[String]) -> Vec<DeviceVoice> {
    voices.iter().filter(|voice| voice_matches_languages(voice, languages)).cloned().collect()
}

pub fn preview_text(language: &str) -> &'static str {
    match language_base(language).as_str() {
        "sv" => "Det här är ett exempel på hur den här rösten läser din bok.",
        "de" => "Dies ist ein Beispiel dafür, wie diese Stimme Ihr Buch vorliest.",
        "fr" => "Voici un exemple de cette voix pour lire votre livre.",
        "es" => "Este es un ejemplo de cómo esta voz lee tu libro.",
        "it" => "Questo è un esempio di come questa voce legge il tuo libro.",
        "pt" => "Este é um exemplo de como esta voz lê o seu livro.",
        _ => "This is a preview of the selected reading voice.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn voice(language: &str) -> DeviceVoice {
        DeviceVoice { id: language.into(), name: language.into(), language: language.into() }
    }

    #[test]
    fn regional_and_iso_languages_match_without_selecting_another_language() {
        assert!(voice_matches_languages(&voice("sv-SE"), &["swe".into()]));
        assert!(voice_matches_languages(&voice("en-US"), &["en-GB".into()]));
        assert!(!voice_matches_languages(&voice("en-US"), &["sv".into()]));
        assert!(voice_matches_languages(&voice("sv-SE"), &["und".into()]));
        assert!(!voice_matches_languages(&voice("en-US"), &["und".into(), "sv".into()]));
    }
}
