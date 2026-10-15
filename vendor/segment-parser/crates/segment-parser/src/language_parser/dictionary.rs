use hunspell_rs::Hunspell;

pub(crate) fn load_bundled_dictionary(lang_tag: &str) -> Option<Hunspell> {
    let paths = hunspell_dictionaries::resolve(lang_tag).ok().flatten()?;
    Some(Hunspell::new(
        paths.aff().to_str().expect("aff path"),
        paths.dic().to_str().expect("dic path"),
    ))
}

/// Dictionary membership is decided by `check` alone — deliberately *not* by
/// `stem`.
///
/// `check` honours the dictionary's own exclusion flags; `stem` ignores them.
/// Swedish marks common misspellings and bad compound formations
/// `FORBIDDENWORD` (`aftonblodet/%`, `agenturna/%`, `afghanstan/%` — `%` is
/// `FORBIDDENWORD` in `sv_SE.aff`), and German marks bound compound-linking
/// forms `NEEDAFFIX` (`Abbildungs/hij` — `h` is `NEEDAFFIX` in `de_DE.aff`).
/// Every one of those is an explicit "this is not a standalone word" from the
/// dictionary author.
///
/// Measured over the installed dictionaries (4000 entries each, plus their
/// uppercase forms and light mutations), accepting `stem` results admitted 778
/// German and 36 Swedish such forms, which suppressed the QA unknown-word
/// check on genuine errors. Do not reinstate `stem` here — it overrides the
/// dictionary rather than reading it. (`stem` remains correct in
/// `dictionary_baseform_with_hunspell`, whose job *is* to find a root.)
pub(crate) fn is_dictionary_word_with_hunspell(hunspell: Option<&Hunspell>, word: &str) -> bool {
    let Some(hunspell) = hunspell else {
        return false;
    };
    if word.is_empty() {
        return false;
    }

    if hunspell.check(word) == hunspell_rs::CheckResult::FoundInDictionary {
        return true;
    }

    let lower = word.to_lowercase();
    let has_lowercase = word.chars().any(|c| c.is_lowercase());
    lower != word && has_lowercase && hunspell.check(&lower) == hunspell_rs::CheckResult::FoundInDictionary
}

pub(crate) fn dictionary_baseform_with_hunspell(hunspell: Option<&Hunspell>, word: &str) -> Option<String> {
    let hunspell = hunspell?;
    if word.is_empty() {
        return None;
    }
    let has_lowercase = word.chars().any(|c| c.is_lowercase());
    hunspell
        .stem(word)
        .into_iter()
        .find(|stem| !stem.is_empty())
        .or_else(|| {
            let lower = word.to_lowercase();
            (lower != word && has_lowercase)
                .then(|| hunspell.stem(&lower).into_iter().find(|stem| !stem.is_empty()))
                .flatten()
        })
}

/// Reduces Hunspell's morphological analyses to lemmas and compound components.
///
/// Readings are `st:` (lemma), `po:` (part of speech, present only for full
/// SALDO entries in the bundled Swedish dictionary) and `cp:` (descriptor-
/// confirmed compound part lemmas). Runtime compounds come back as `pa:` parts,
/// each followed by its own fields; only the readings with the fewest parts
/// count, since Hunspell also lists junk splits (`kont`+`ors`+`planering`).
/// A part Hunspell reports without a lemma is analysed on its own. Values are stored in
/// terminology-normalised form so term matching can compare them without allocating.
pub(crate) fn dictionary_analysis_with_hunspell(hunspell: Option<&Hunspell>, word: &str) -> Option<structure::DictionaryAnalysis> {
    let hunspell = hunspell?;
    let readings = hunspell_readings(hunspell, word);
    if readings.is_empty() {
        return None;
    }
    let fewest_parts = readings.iter().filter(|reading| reading.is_compound()).map(|reading| reading.parts.len()).min();
    let (simple, compound): (Vec<_>, Vec<_>) = readings.iter().partition(|reading| !reading.is_compound());
    let lemmas = simple.iter().flat_map(|reading| reading.word.lemmas.iter().cloned());
    let listed_parts = simple.iter().flat_map(|reading| reading.word.compound_parts.iter().cloned());
    let runtime_parts = compound.iter().filter(|reading| Some(reading.parts.len()) == fewest_parts).flat_map(|reading| reading.parts.iter()).flat_map(|part| part_components(hunspell, part));
    let authoritative = simple.iter().any(|reading| reading.word.has_part_of_speech);
    Some(structure::DictionaryAnalysis::new(distinct(lemmas), distinct(listed_parts.chain(runtime_parts)), authoritative))
}

fn hunspell_readings(hunspell: &Hunspell, word: &str) -> Vec<HunspellReading> {
    let analyses = hunspell.analyze(word);
    let analyses = if analyses.is_empty() && word.chars().any(char::is_uppercase) && word.chars().any(char::is_lowercase) { hunspell.analyze(&word.to_lowercase()) } else { analyses };
    analyses.iter().map(|analysis| HunspellReading::parse(analysis)).collect()
}

fn part_components(hunspell: &Hunspell, part: &HunspellPart) -> Vec<String> {
    let own = std::iter::once(part.surface.clone()).chain(part.fields.lemmas.iter().cloned()).chain(part.fields.compound_parts.iter().cloned());
    let looked_up = part.fields.lemmas.is_empty().then(|| hunspell_readings(hunspell, &part.surface)).unwrap_or_default().into_iter().filter(|reading| !reading.is_compound()).flat_map(|reading| reading.word.lemmas.into_iter().chain(reading.word.compound_parts));
    own.chain(looked_up).collect()
}

fn distinct(values: impl Iterator<Item = String>) -> Vec<String> { values.map(|value| crate::canonical_search_text(&value)).fold(Vec::new(), |mut distinct, value| { if !distinct.contains(&value) { distinct.push(value); } distinct }) }

#[derive(Default)]
struct HunspellFields {
    lemmas: Vec<String>,
    compound_parts: Vec<String>,
    has_part_of_speech: bool,
    /// The current alternative has an affix flag before its stem: a prefix was stripped.
    prefixed: bool,
    stem_seen: bool,
}

impl HunspellFields {
    /// Hunspell prints a prefix's `fl:` before the stem and suffix flags after it. A stem reached by stripping a
    /// prefix is no lemma of the word (`reports` → `fl:A st:port fl:S`), so it is dropped.
    fn add(&mut self, field: &str, value: &str) {
        match field {
            "st" if !self.prefixed => self.lemmas.push(value.to_string()),
            "fl" if !self.stem_seen => self.prefixed = true,
            "cp" => self.compound_parts.push(value.to_string()),
            "po" => self.has_part_of_speech = true,
            _ => {}
        }
        self.stem_seen |= field == "st";
    }
    fn begin_alternative(&mut self) {
        self.prefixed = false;
        self.stem_seen = false;
    }
}

struct HunspellPart {
    surface: String,
    fields: HunspellFields,
}

struct HunspellReading {
    word: HunspellFields,
    parts: Vec<HunspellPart>,
}

impl HunspellReading {
    /// Alternative groups (`( st:a | st:b )`) are flattened: every alternative's fields belong to the current part.
    fn parse(analysis: &str) -> Self {
        let mut reading = analysis.split_whitespace().fold(Self { word: HunspellFields::default(), parts: Vec::new() }, |mut reading, token| {
            match (token.split_once(':'), reading.parts.last_mut()) {
                (Some(("pa", value)), _) => reading.parts.push(HunspellPart { surface: value.to_string(), fields: HunspellFields::default() }),
                (Some((field, value)), Some(part)) => part.fields.add(field, value),
                (Some((field, value)), None) => reading.word.add(field, value),
                (None, part) if matches!(token, "(" | "|") => part.map_or(&mut reading.word, |part| &mut part.fields).begin_alternative(),
                (None, _) => {}
            }
            reading
        });
        if let [part] = reading.parts.as_mut_slice() {
            let fields = std::mem::take(&mut part.fields);
            reading.word.lemmas.extend(fields.lemmas);
            reading.word.compound_parts.extend(fields.compound_parts);
            reading.word.has_part_of_speech |= fields.has_part_of_speech;
            reading.parts.clear();
        }
        reading
    }
    fn is_compound(&self) -> bool { self.parts.len() >= 2 }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forms the dictionary author explicitly excluded — Swedish
    /// `FORBIDDENWORD`, German `NEEDAFFIX` — are stemmable but are not words.
    /// The dictionaries are bundled, so this runs identically on every host.
    #[test]
    fn stemmable_but_unchecked_forms_are_not_dictionary_words() {
        let Some(de) = load_bundled_dictionary("de_DE") else { return };
        ["Haus", "Flugzeug", "Bahnhof"].iter().for_each(|word| assert!(is_dictionary_word_with_hunspell(Some(&de), word), "{word} is a listed word"));
        assert!(!is_dictionary_word_with_hunspell(Some(&de), "Abbildungs"), "NEEDAFFIX bound form is not a standalone word");

        let Some(sv) = load_bundled_dictionary("sv_SE") else { return };
        ["bokhylla", "sjukhus", "flygplan"].iter().for_each(|word| assert!(is_dictionary_word_with_hunspell(Some(&sv), word), "{word} is a listed word"));
        ["aftonblodet", "agenturna"].iter().for_each(|word| assert!(!is_dictionary_word_with_hunspell(Some(&sv), word), "{word} is FORBIDDENWORD in sv_SE.dic"));
    }

    #[test]
    fn hunspell_readings_split_runtime_compound_parts_and_flatten_alternatives() {
        let reading = HunspellReading::parse("pa:fönsterputsar st:fönsterputsare po:nn pa:företaget  (  st:företag po:nn fl:B |  st:företa po:vb fl:C ) ");
        assert_eq!(reading.parts.iter().map(|part| part.surface.as_str()).collect::<Vec<_>>(), ["fönsterputsar", "företaget"]);
        assert_eq!(reading.parts[1].fields.lemmas, ["företag", "företa"]);
        assert!(HunspellReading::parse(" fl:A st:port fl:S").word.lemmas.is_empty(), "prefix-stripped stem is not a lemma");
        assert_eq!(HunspellReading::parse(" ( st:watch fl:D | fl:A st:atch ) ").word.lemmas, ["watch"]);
        let listed = HunspellReading::parse(" st:kontorsstol po:nn cp:kontor cp:stol fl:B");
        assert!(!listed.is_compound() && listed.word.has_part_of_speech);
        assert_eq!(listed.word.compound_parts, ["kontor", "stol"]);
    }

    /// The bundled Swedish dictionary carries SALDO lemmas, parts of speech and confirmed compound parts.
    #[test]
    fn swedish_analysis_reports_all_lemmas_and_compound_components() {
        let Some(sv) = load_bundled_dictionary("sv_SE") else { return };
        let planet = dictionary_analysis_with_hunspell(Some(&sv), "planet").expect("planet is analysed");
        assert!(planet.is_authoritative());
        assert!(["planet", "plan"].iter().all(|lemma| planet.lemmas().any(|candidate| candidate == *lemma)));
        let chair = dictionary_analysis_with_hunspell(Some(&sv), "kontorsstolen").expect("kontorsstolen is analysed");
        assert!(["kontor", "stol"].iter().all(|part| chair.components().any(|candidate| candidate == *part)));
        let planning = dictionary_analysis_with_hunspell(Some(&sv), "kontorsplanering").expect("kontorsplanering is analysed");
        assert!(!planning.is_authoritative());
        assert!(["kontor", "planering"].iter().all(|part| planning.components().any(|candidate| candidate == *part)));
        assert!(!planning.components().any(|candidate| candidate == "kont"), "junk splits with more parts are ignored");
    }

    #[test]
    fn bundled_dictionaries_accept_locale_and_iso_language_tags() {
        ["sv-SE", "swe", "de_de", "deu", "ger", "en_GB-large", "eng"].into_iter().for_each(|tag| assert!(load_bundled_dictionary(tag).is_some(), "missing bundled dictionary for {tag}"));
        assert!(load_bundled_dictionary("sr-Latn-RS").is_none());
    }

    #[test]
    fn bare_supported_language_tags_use_bundled_regional_dictionaries() {
        [("en", "en_GB"), ("sv", "sv_SE"), ("de", "de_DE")].into_iter().for_each(|(bare, regional)| {
            assert!(load_bundled_dictionary(regional).is_some());
            assert!(load_bundled_dictionary(bare).is_some(), "{bare} did not resolve like {regional}");
        });
    }
}
