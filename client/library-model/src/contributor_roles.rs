use crate::MarcRelatorCode;

pub const AUTHOR: MarcRelatorCode = MarcRelatorCode(*b"aut");
pub const CONTRIBUTOR: MarcRelatorCode = MarcRelatorCode(*b"ctb");

fn parse_three_letter_code(value: &str) -> Option<MarcRelatorCode> {
    let bytes = value.as_bytes();
    if bytes.len() != 3 || !bytes.iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    Some(MarcRelatorCode([bytes[0].to_ascii_lowercase(), bytes[1].to_ascii_lowercase(), bytes[2].to_ascii_lowercase()]))
}

/// Converts role evidence from an input metadata format into the canonical
/// MARC vocabulary. Unknown format-specific values conservatively become the
/// general contributor role instead of entering synchronized state.
///
/// This normalization is a client-side metadata-ingestion concern: sync and
/// storage only ever carry the resulting three-byte [`MarcRelatorCode`].
pub fn marc_relator_code_from_metadata_role(value: &str) -> MarcRelatorCode {
    let trimmed = value.trim();
    let token = trimmed.rsplit(['/', '#', ':']).next().unwrap_or(trimmed).trim();
    if let Some(code) = parse_three_letter_code(token) {
        return code;
    }
    let normalized = token.chars().filter(|character| character.is_ascii_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
    let bytes = match normalized.as_str() {
        "author" | "writer" => return AUTHOR,
        "adapter" => b"adp",
        "annotator" => b"ann",
        "artist" => b"art",
        "bookproducer" => b"bkp",
        "blurbwriter" => b"blw",
        "compiler" => b"com",
        "coverartist" | "coverdesigner" => b"cov",
        "editor" => b"edt",
        "illustrator" => b"ill",
        "interviewee" => b"ive",
        "markupeditor" => b"mrk",
        "narrator" => b"nrt",
        "owner" => b"own",
        "performer" => b"prf",
        "proofreader" => b"pfr",
        "sponsor" => b"spn",
        "transcriber" => b"trc",
        "translator" => b"trl",
        "typedesigner" => b"tyd",
        "typographer" => b"tyg",
        "voiceactor" => b"vac",
        _ => return CONTRIBUTOR,
    };
    MarcRelatorCode(*bytes)
}

/// Rich, human-facing interpretation of a synced [`MarcRelatorCode`].
///
/// Persistence and sync code only ever needs the raw three-byte code;
/// this classification exists for client presentation (grouping, labelling,
/// sorting contributors) and has no bearing on sync/storage correctness.
pub const fn classify(code: MarcRelatorCode) -> ContributorRole {
    match &code.0 {
        b"aut" => ContributorRole::Author,
        b"adp" => ContributorRole::Content(ContentRole::Adapter),
        b"ann" => ContributorRole::Content(ContentRole::Annotator),
        b"com" => ContributorRole::Content(ContentRole::Compiler),
        b"ctb" => ContributorRole::Content(ContentRole::GeneralContributor),
        b"edc" => ContributorRole::Content(ContentRole::CompilationEditor),
        b"edt" => ContributorRole::Content(ContentRole::Editor),
        b"ive" => ContributorRole::Content(ContentRole::Interviewee),
        b"trl" => ContributorRole::Content(ContentRole::Translator),
        b"aui" | b"win" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::Introduction)),
        b"wfw" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::Foreword)),
        b"wpr" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::Preface)),
        b"wac" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::Commentary)),
        b"wat" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::AddedText)),
        b"wst" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::SupplementaryText)),
        b"blw" => ContributorRole::Content(ContentRole::SupplementaryWriter(SupplementaryWriterRole::Blurb)),
        b"art" => ContributorRole::Visual(VisualRole::Artist),
        b"cov" => ContributorRole::Visual(VisualRole::CoverDesigner),
        b"ill" => ContributorRole::Visual(VisualRole::Illustrator),
        b"nrt" => ContributorRole::Performance(PerformanceRole::Narrator),
        b"prf" => ContributorRole::Performance(PerformanceRole::Performer),
        b"vac" => ContributorRole::Performance(PerformanceRole::VoiceActor),
        b"bkp" => ContributorRole::Production(ProductionRole::BookProducer),
        b"mrk" => ContributorRole::Production(ProductionRole::MarkupEditor),
        b"pfr" => ContributorRole::Production(ProductionRole::Proofreader),
        b"trc" => ContributorRole::Production(ProductionRole::Transcriber),
        b"tyd" => ContributorRole::Production(ProductionRole::TypeDesigner),
        b"tyg" => ContributorRole::Production(ProductionRole::Typographer),
        b"spn" => ContributorRole::Support(SupportRole::Sponsor),
        b"own" => ContributorRole::Provenance(ProvenanceRole::Owner),
        _ => ContributorRole::Other(code),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContributorRole {
    Author,
    Content(ContentRole),
    Visual(VisualRole),
    Performance(PerformanceRole),
    Production(ProductionRole),
    Support(SupportRole),
    Provenance(ProvenanceRole),
    Other(MarcRelatorCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentRole {
    Adapter,
    Annotator,
    Compiler,
    CompilationEditor,
    Editor,
    GeneralContributor,
    Interviewee,
    Translator,
    SupplementaryWriter(SupplementaryWriterRole),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupplementaryWriterRole {
    Introduction,
    Foreword,
    Preface,
    Commentary,
    AddedText,
    SupplementaryText,
    Blurb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualRole {
    Artist,
    CoverDesigner,
    Illustrator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerformanceRole {
    Narrator,
    Performer,
    VoiceActor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionRole {
    BookProducer,
    MarkupEditor,
    Proofreader,
    Transcriber,
    TypeDesigner,
    Typographer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupportRole {
    Sponsor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvenanceRole {
    Owner,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_vocabularies_normalize_once_to_marc() {
        assert_eq!(marc_relator_code_from_metadata_role("translator"), MarcRelatorCode(*b"trl"));
        assert_eq!(marc_relator_code_from_metadata_role("http://id.loc.gov/vocabulary/relators/EDT"), MarcRelatorCode(*b"edt"));
        assert_eq!(marc_relator_code_from_metadata_role("marc:relators:ill"), MarcRelatorCode(*b"ill"));
        assert_eq!(marc_relator_code_from_metadata_role("vendor-specific-role"), CONTRIBUTOR);
    }

    #[test]
    fn rich_roles_are_derived_without_changing_canonical_storage() {
        let translator = MarcRelatorCode(*b"trl");
        assert_eq!(classify(translator), ContributorRole::Content(ContentRole::Translator));
        let future_code = MarcRelatorCode(*b"xyz");
        assert_eq!(classify(future_code), ContributorRole::Other(future_code));
    }
}
