use std::str;

use serde::{Deserialize, Serialize};

/// Opaque MARC relator code carried by persistence and synchronization.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MarcRelatorCode(pub [u8; 3]);

pub const AUTHOR_MARC_RELATOR_CODE: MarcRelatorCode = MarcRelatorCode(*b"aut");
pub const CONTRIBUTOR_MARC_RELATOR_CODE: MarcRelatorCode = MarcRelatorCode(*b"ctb");
pub const NARRATOR_MARC_RELATOR_CODE: MarcRelatorCode = MarcRelatorCode(*b"nrt");

impl MarcRelatorCode {
    pub fn as_str(&self) -> &str {
        str::from_utf8(&self.0).unwrap_or("ctb")
    }
}

fn parse_three_letter_code(value: &str) -> Option<MarcRelatorCode> {
    let bytes = value.as_bytes();
    if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    Some(MarcRelatorCode([bytes[0].to_ascii_lowercase(), bytes[1].to_ascii_lowercase(), bytes[2].to_ascii_lowercase()]))
}

/// Recognizes author and narrator labels, preserving explicit MARC codes.
/// Other labels use the generic contributor code.
pub fn marc_relator_code_from_metadata_role(value: &str) -> MarcRelatorCode {
    let trimmed = value.trim();
    let token = trimmed.rsplit(['/', '#', ':']).next().unwrap_or(trimmed).trim();
    if let Some(code) = parse_three_letter_code(token) {
        return code;
    }
    let normalized = token.chars().filter(|character| character.is_ascii_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    match normalized.as_str() {
        "author" | "writer" => AUTHOR_MARC_RELATOR_CODE,
        "narrator" => NARRATOR_MARC_RELATOR_CODE,
        _ => CONTRIBUTOR_MARC_RELATOR_CODE,
    }
}

#[cfg(test)]
mod tests {
    use super::{marc_relator_code_from_metadata_role, MarcRelatorCode, AUTHOR_MARC_RELATOR_CODE, CONTRIBUTOR_MARC_RELATOR_CODE, NARRATOR_MARC_RELATOR_CODE};

    #[test]
    fn author_and_narrator_roles_accept_labels_codes_and_uris() {
        for value in ["Author", " writer ", "AUT", "urn:marc:aut", "http://id.loc.gov/vocabulary/relators/aut"] {
            assert_eq!(marc_relator_code_from_metadata_role(value), AUTHOR_MARC_RELATOR_CODE, "{value}");
        }
        for value in ["Narrator", " NRT ", "urn:marc:nrt", "http://id.loc.gov/vocabulary/relators/nrt"] {
            assert_eq!(marc_relator_code_from_metadata_role(value), NARRATOR_MARC_RELATOR_CODE, "{value}");
        }
    }

    #[test]
    fn other_role_labels_use_generic_contributor_code() {
        for value in ["editor", "translator", "illustrator", "book producer", "voice actor", "", "unknown role"] {
            assert_eq!(marc_relator_code_from_metadata_role(value), CONTRIBUTOR_MARC_RELATOR_CODE, "{value}");
        }
    }

    #[test]
    fn explicit_other_marc_codes_are_preserved() {
        assert_eq!(marc_relator_code_from_metadata_role("TRL"), MarcRelatorCode(*b"trl"));
        assert_eq!(marc_relator_code_from_metadata_role("http://id.loc.gov/vocabulary/relators/edt"), MarcRelatorCode(*b"edt"));
    }
}
