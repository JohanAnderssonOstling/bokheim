#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentifierScheme {
    ISBN,
    ASIN,
    GOOG,
    GOODREADS,
    KOBO,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreatorRole {
    Author,
    Editor,
    Translator,
    Illustrator,
    Narrator,
    Other(String),
}

pub fn parse_identifier_scheme(scheme: &str) -> Option<IdentifierScheme> {
    match scheme.to_uppercase().as_str() {
        "ISBN" => Some(IdentifierScheme::ISBN),
        "ASIN" => Some(IdentifierScheme::ASIN),
        "MOBI-ASIN" => Some(IdentifierScheme::ASIN),
        "AMAZON" => Some(IdentifierScheme::ASIN),
        "OASIN" => Some(IdentifierScheme::ASIN),
        "GOODREADS" => Some(IdentifierScheme::GOODREADS),
        "GOOG" | "GOOGLE" => Some(IdentifierScheme::GOOG),
        "KOBO" => Some(IdentifierScheme::KOBO),
        "UUID" => None,
        "CALIBRE" => None,
        other => {
            if other.contains("ISBN") {
                Some(IdentifierScheme::ISBN)
            } else {
                Some(IdentifierScheme::Other(String::from(other)))
            }
        }
    }
}

pub fn parse_creator_role(role: &str) -> CreatorRole {
    match role.to_lowercase().as_str() {
        "aut" | "author" => CreatorRole::Author,
        "edt" | "editor" => CreatorRole::Editor,
        "trl" | "translator" => CreatorRole::Translator,
        "ill" | "illustrator" => CreatorRole::Illustrator,
        "nrt" | "narrator" => CreatorRole::Narrator,
        other => CreatorRole::Other(other.to_string()),
    }
}
