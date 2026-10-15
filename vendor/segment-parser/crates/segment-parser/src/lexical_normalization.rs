use rust_stemmers::{Algorithm, Stemmer};

pub fn is_stopword(token: &str, language: Option<&str>) -> bool {
    language.map(primary_language).unwrap_or("en").eq_ignore_ascii_case("en") && matches!(
        token,
        "a" | "all" | "an" | "and" | "are" | "as" | "at" | "be" | "been" | "by" | "can" | "for" | "from" | "have" | "his" | "in" | "is" | "it" | "just" | "of" | "on" | "or" | "the" | "to" | "where" | "with" | "you"
    )
}

pub fn stem(token: &str) -> String { stem_for_language(token, "en") }

pub fn stem_for_language(token: &str, language: &str) -> String {
    if !token.chars().all(char::is_alphabetic) { return token.to_string(); }
    let Some(algorithm) = stemmer_algorithm(primary_language(language)) else { return token.to_string(); };
    Stemmer::create(algorithm).stem(token).into_owned()
}

fn primary_language(language: &str) -> &str { language.split(['-', '_']).next().unwrap_or(language) }

fn stemmer_algorithm(language: &str) -> Option<Algorithm> {
    match language.to_ascii_lowercase().as_str() {
        "ar" => Some(Algorithm::Arabic),
        "da" => Some(Algorithm::Danish),
        "nl" => Some(Algorithm::Dutch),
        "en" => Some(Algorithm::English),
        "fi" => Some(Algorithm::Finnish),
        "fr" => Some(Algorithm::French),
        "de" => Some(Algorithm::German),
        "el" => Some(Algorithm::Greek),
        "hu" => Some(Algorithm::Hungarian),
        "it" => Some(Algorithm::Italian),
        "no" | "nb" | "nn" => Some(Algorithm::Norwegian),
        "pt" => Some(Algorithm::Portuguese),
        "ro" => Some(Algorithm::Romanian),
        "ru" => Some(Algorithm::Russian),
        "es" => Some(Algorithm::Spanish),
        "sv" => Some(Algorithm::Swedish),
        "ta" => Some(Algorithm::Tamil),
        "tr" => Some(Algorithm::Turkish),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stemming_uses_the_primary_language_subtag() { assert_eq!(stem_for_language("cars", "en-GB"), "car"); }

    #[test]
    fn unsupported_and_non_alphabetic_tokens_are_unchanged() { assert_eq!(stem_for_language("cars", "zz"), "cars"); assert_eq!(stem_for_language("A-10", "en"), "A-10"); }

    #[test]
    fn stopwords_are_currently_an_english_policy() { assert!(is_stopword("the", Some("en-GB"))); assert!(!is_stopword("the", Some("sv-SE"))); }
}
