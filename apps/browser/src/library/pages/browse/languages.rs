//! Language choices shared by library and catalog browsing. Preserve stored
//! metadata codes while treating two- and three-letter aliases as one choice.
use ui_text::text;

use library_model::LibraryLanguageCount;
use std::collections::{BTreeMap, HashSet};

const LANGUAGES: &[(&str, &str, &[&str])] = &[
    ("en", text!(common::english), &["eng"]),
    ("sv", text!(common::swedish), &["swe"]),
    ("no", text!(browse::norwegian), &["nor"]),
    ("nb", text!(common::norwegian_bokmal), &["nob"]),
    ("nn", text!(common::norwegian_nynorsk), &["nno"]),
    ("da", text!(common::danish), &["dan"]),
    ("de", text!(common::german), &["deu", "ger"]),
    ("fr", text!(common::french), &["fra", "fre"]),
    ("es", text!(common::spanish), &["spa"]),
    ("it", text!(common::italian), &["ita"]),
    ("nl", text!(common::dutch), &["nld", "dut"]),
    ("pt", text!(common::portuguese), &["por"]),
    ("pl", text!(common::polish), &["pol"]),
    ("he", text!(common::hebrew), &["heb"]),
    ("ja", text!(common::japanese), &["jpn"]),
    ("el", text!(common::greek), &["ell", "gre"]),
    ("tl", text!(common::tagalog), &["tgl"]),
    ("bg", text!(common::bulgarian), &["bul"]),
    ("ta", text!(common::tamil), &["tam"]),
    ("zh", text!(common::chinese), &["zho", "chi"]),
    ("se", text!(browse::northern_sami), &["sme"]),
    ("fi", text!(common::finnish), &["fin"]),
    ("is", text!(common::icelandic), &["isl", "ice"]),
    ("ru", text!(common::russian), &["rus"]),
    ("uk", text!(common::ukrainian), &["ukr"]),
    ("ar", text!(common::arabic), &["ara"]),
    ("hi", text!(common::hindi), &["hin"]),
    ("la", text!(common::latin), &["lat"]),
    ("ang", text!(browse::old_english), &[]),
    ("smi", text!(browse::sami_languages), &[]),
    ("mul", text!(browse::multiple_languages), &[]),
    ("und", text!(browse::unspecified_language), &[]),
    ("zxx", text!(browse::no_linguistic_content), &[]),
];

fn canonical(code: &str) -> String {
    let code = code.trim().split(['-', '_', '.', '@']).next().unwrap_or_default().to_ascii_lowercase();
    LANGUAGES.iter().find(|(primary, _, aliases)| *primary == code || aliases.contains(&code.as_str())).map_or_else(|| code, |(primary, _, _)| (*primary).to_owned())
}

pub(super) fn choices(counts: &[LibraryLanguageCount]) -> Vec<(String, String, u32)> {
    let mut languages = BTreeMap::<String, u32>::new();
    for count in counts {
        let code = canonical(&count.language);
        if !code.is_empty() {
            *languages.entry(code).or_default() += count.book_count;
        }
    }
    let mut choices = languages
        .into_iter()
        .map(|(code, count)| {
            let label = LANGUAGES.iter().find(|(primary, _, _)| *primary == code).map_or_else(|| code.clone(), |(_, label, _)| (*label).to_owned());
            (code, label, count)
        })
        .collect::<Vec<_>>();
    choices.sort_unstable_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    choices
}

pub(super) fn query_codes(selected: &[String]) -> Vec<String> {
    let mut codes = Vec::new();
    for language in selected {
        let code = canonical(language);
        if let Some((_, _, aliases)) = LANGUAGES.iter().find(|(primary, _, _)| *primary == code) {
            codes.extend(aliases.iter().map(|alias| (*alias).to_owned()));
        }
        codes.push(code);
    }
    codes.sort_unstable();
    codes.dedup();
    codes
}

pub(super) fn infer(system: &[String], library: &[LibraryLanguageCount], catalog: &[LibraryLanguageCount]) -> Vec<String> {
    let available: HashSet<_> = catalog.iter().filter(|count| count.book_count > 0).map(|count| canonical(&count.language)).collect();
    let mut selected = system
        .iter()
        .map(|language| canonical(language))
        .chain(library.iter().filter(|count| count.book_count > 0).map(|count| canonical(&count.language)))
        .filter(|language| language.len() >= 2 && language.len() <= 3 && language.bytes().all(|byte| byte.is_ascii_alphabetic()) && !matches!(language.as_str(), "mul" | "und" | "zxx"))
        .filter(|language| available.contains(language))
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected.dedup();
    if !selected.iter().any(|language| language == "en") {
        selected.push("en".into());
    }
    selected
}

/// Infer once, then use the saved selection even when library or system
/// languages change. An explicit empty selection continues to mean Any.
pub(super) fn initial_selection(saved: Option<Vec<String>>, system: &[String], library: &[LibraryLanguageCount], catalog: &[LibraryLanguageCount]) -> Vec<String> {
    saved.unwrap_or_else(|| infer(system, library, catalog))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(language: &str, book_count: u32) -> LibraryLanguageCount {
        LibraryLanguageCount { language: language.into(), book_count }
    }

    #[test]
    fn actual_languages_include_aliases_and_choices_outside_the_old_menu() {
        let choices = choices(&[count("en-US", 2), count("eng", 3), count("sv", 4), count("heb", 1), count("ang", 1), count("cy", 2)]);
        assert!(choices.contains(&("en".into(), "English".into(), 5)));
        assert!(choices.contains(&("he".into(), "Hebrew".into(), 1)));
        assert!(choices.contains(&("ang".into(), "Old English".into(), 1)));
        assert!(choices.iter().any(|(code, _, _)| code == "cy"));
        assert_eq!(query_codes(&["sv".into(), "en".into()]), ["en", "eng", "sv", "swe"]);
    }

    #[test]
    fn defaults_combine_system_and_library_without_guessing_unknowns() {
        let catalog = [count("en", 100), count("sv", 1522), count("fr", 5), count("mul", 1)];
        let library = [count("swe", 20), count("und", 5), count("mul", 1), count("fra", 0), count("jpn", 3)];
        assert_eq!(infer(&["C.UTF-8".into(), "en_US.UTF-8".into()], &library, &catalog), ["en", "sv"]);
        assert_eq!(infer(&["ja-JP".into()], &[], &catalog), ["en"]);
    }

    #[test]
    fn defaults_use_positive_catalog_aliases_and_append_the_english_fallback() {
        let catalog = [count("SWE", 0), count("sv-SE", 2), count("jpn", 0), count("en-US", 0), count("fra-FR", 1)];
        let library = [count("swe", 4), count("jpn", 6), count("fra", 0)];
        assert_eq!(infer(&["sv_SE.UTF-8".into(), "ja-JP".into(), "fr-FR".into()], &library, &catalog), ["fr", "sv", "en"]);
        let saved = vec!["swe".into(), "EN_us.UTF-8".into(), "unknown".into()];
        assert_eq!(initial_selection(Some(saved.clone()), &[], &library, &catalog), saved);
    }

    #[test]
    fn saved_default_and_any_are_kept_when_system_or_library_languages_change() {
        let catalog = [count("en", 100), count("sv", 1522), count("fr", 5)];
        let initial = initial_selection(None, &["en-US".into()], &[count("swe", 2)], &catalog);
        assert_eq!(initial, ["en", "sv"]);
        assert_eq!(initial_selection(Some(initial), &["fr-FR".into()], &[count("fra", 10)], &catalog), ["en", "sv"]);
        assert_eq!(initial_selection(Some(vec!["sv".into()]), &["en-US".into()], &[count("eng", 20)], &catalog), ["sv"]);
        assert!(initial_selection(Some(vec![]), &["fr-FR".into()], &[count("swe", 20)], &catalog).is_empty());
    }
}
