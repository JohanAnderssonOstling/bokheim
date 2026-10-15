//! Scoped, reviewed raw headings select pinned routes in the curated taxonomy.
use super::{bisac_code_for_path, canonical_subject_system, compact_whitespace, match_bisac_subjects, unified_concept_id, unified_subject_paths, BookSubject, BISAC_SYSTEM_ID, UNIFIED_SYSTEM_ID};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub(crate) const SOURCE_HEADING_PREFIX: &str = "metadata:source-heading:";
pub(crate) const GENERIC_DRAMA_PATH: &str = "Literature / Genres / Literary Forms / Drama";
const GENERIC_DRAMA_CONCEPT_ID: i64 = 3876;
pub(crate) const GENERAL_FICTION_PATH: &str = "Literature";
pub(crate) const GENERAL_FICTION_CONCEPT_ID: i64 = 3846;

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MatchScope {
    #[default]
    ExactHeading,
    FinalForm,
    PrimaryComponent,
    TopicComponent,
    TopicSubdivision,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeadingRule {
    #[serde(default)]
    scope: MatchScope,
    raw: String,
    #[serde(default)]
    required_sources: Vec<String>,
    concept_id: i64,
    destination: String,
}

impl HeadingRule {
    fn permits_source(&self, source: &str) -> bool {
        self.required_sources.is_empty() || self.required_sources.iter().any(|required| required == source)
    }
}

fn heading_key(raw: &str) -> String {
    // Subdivisions, punctuation and dates are part of the reviewed input.
    compact_whitespace(raw).to_lowercase()
}

fn rules() -> &'static BTreeMap<(MatchScope, String), Option<HeadingRule>> {
    static RULES: OnceLock<BTreeMap<(MatchScope, String), Option<HeadingRule>>> = OnceLock::new();
    RULES.get_or_init(|| {
        let rules: Vec<HeadingRule> = serde_json::from_str(include_str!("../data/reviewed-source-headings.json")).expect("reviewed source-heading rules must be valid");
        let mut by_heading = BTreeMap::new();
        for rule in rules {
            // A duplicate within one scope fails closed; the same token can
            // legitimately have distinct exact-heading and component scopes.
            by_heading.entry((rule.scope, heading_key(&rule.raw))).and_modify(|entry| *entry = None).or_insert(Some(rule));
        }
        by_heading
    })
}

/// Returns a pinned exact route, or a broad form/topic subdivision fallback.
/// Complete matches take priority; conflicting topical fallbacks stay unresolved.
/// Derived enrichment subjects must still agree with their original heading.
pub fn reviewed_source_heading_path(subject: &BookSubject) -> Option<&'static str> {
    let derived = subject.authority() == Some(UNIFIED_SYSTEM_ID);
    let source = if derived {
        subject.source().strip_prefix(SOURCE_HEADING_PREFIX)?
    } else {
        if subject.authority().is_some_and(|authority| canonical_subject_system(authority) != Some("lcsh")) {
            return None;
        }
        subject.source()
    };
    if !matches!(source, "dc:subject" | "dcterms:subject" | "pdf:xmp:dc:subject" | "calibre:user_metadata/#fast") && !source.starts_with("catalog:") {
        return None;
    }
    let heading = heading_key(subject.name());
    let (path, concept_id) = if let Some(rule) = rules().get(&(MatchScope::ExactHeading, heading.clone())) {
        // A conflicting or stale exact rule must not fall through to a form.
        let rule = rule.as_ref()?;
        if !rule.permits_source(source) {
            return None;
        }
        (rule.destination.as_str(), rule.concept_id)
    } else {
        // Evaluate the original declaration, not a materialized unified code.
        let original = if derived { BookSubject::new(None, subject.name(), source, None, None).ok()? } else { subject.clone() };
        if heading.split("--").any(|component| component.trim().is_empty()) {
            return None;
        }
        let fallback = if let Some(target) = subdivision_fallback(&heading, source).ok()? {
            target
        } else if is_drama_form(&heading) {
            (GENERIC_DRAMA_PATH, GENERIC_DRAMA_CONCEPT_ID)
        } else if heading == "fiction" || heading.rsplit_once("--").is_some_and(|(_, form)| form.trim() == "fiction") {
            (GENERAL_FICTION_PATH, GENERAL_FICTION_CONCEPT_ID)
        } else {
            return None;
        };
        if match_bisac_subjects(std::slice::from_ref(&original)).iter().any(|assignment| bisac_code_for_path(assignment.node_path()).is_some_and(|code| !unified_subject_paths(BISAC_SYSTEM_ID, &code).is_empty())) {
            return None;
        }
        fallback
    };
    if derived && subject.code() != Some(path) {
        return None;
    }
    (unified_concept_id(path) == Some(concept_id)).then_some(path)
}

fn subdivision_fallback(heading: &str, source: &str) -> Result<Option<(&'static str, i64)>, ()> {
    let components = heading.split("--").map(str::trim).collect::<Vec<_>>();
    if components.len() < 2 || components.iter().any(|component| component.is_empty()) {
        return Ok(None);
    }
    let last = *components.last().ok_or(())?;
    if last == "juvenile literature" && components.get(components.len() - 2) == Some(&"biography") {
        return Ok(Some(("Youth / Juvenile Nonfiction / People, Society & History / Biography & Autobiography", 1440)));
    }
    // Final forms override earlier topics. A conflicting form definition
    // cannot fall through to a broader primary/topic rule.
    if let Some(rule) = rules().get(&(MatchScope::FinalForm, last.to_owned())) {
        let rule = rule.as_ref().ok_or(())?;
        if !rule.permits_source(source) {
            return Err(());
        }
        return Ok(Some((rule.destination.as_str(), rule.concept_id)));
    }
    if last.starts_with("juvenile") || last.starts_with("young adult") {
        return Err(());
    }
    if components.iter().skip(1).any(|component| matches!(*component, "history and criticism" | "criticism and interpretation")) {
        let primary = components[0];
        if primary == "drama" || primary.ends_with(" drama") {
            return Ok(Some(("Literature / Criticism / Genres & Forms / Drama", 4328)));
        }
        if primary == "poetry" || primary.ends_with(" poetry") {
            return Ok(Some(("Literature / Criticism / Genres & Forms / Poetry", 4355)));
        }
        return Ok(
            (primary == "fiction" || primary.ends_with(" fiction") || primary == "literature" || primary.ends_with(" literature") || matches!(primary, "short stories" | "essays" | "novels")).then_some(("Literature / Criticism", 4309))
        );
    }
    // Do not discard an audience or a later form to force an adult topic.
    if components.iter().skip(1).any(|component| rules().contains_key(&(MatchScope::FinalForm, (*component).to_owned()))) {
        return Err(());
    }
    let mut targets = Vec::new();
    for ((scope, raw), rule) in rules() {
        let matched = match scope {
            MatchScope::PrimaryComponent => components[0] == raw,
            MatchScope::TopicComponent => components.contains(&raw.as_str()),
            MatchScope::TopicSubdivision => components[1..].contains(&raw.as_str()),
            MatchScope::ExactHeading | MatchScope::FinalForm => false,
        };
        if matched {
            let rule = rule.as_ref().ok_or(())?;
            if !rule.permits_source(source) {
                continue;
            }
            // Invalid candidate destinations also fail closed before we
            // consider whether they shadow a broader route.
            if unified_concept_id(&rule.destination) != Some(rule.concept_id) {
                return Err(());
            }
            targets.push((rule.destination.as_str(), rule.concept_id));
        }
    }
    targets.sort_unstable();
    targets.dedup();
    let candidates = targets.iter().filter(|(path, _)| !targets.iter().any(|(other, _)| other.starts_with(&format!("{path} / ")))).collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => Ok(None),
        [target] => Ok(Some(**target)),
        _ => Err(()),
    }
}

fn is_drama_form(heading: &str) -> bool {
    // An explicit final form subdivision describes a dramatic work, even
    // when an earlier subdivision describes its historical subject matter.
    if heading.rsplit_once("--").is_some_and(|(_, form)| form.trim() == "drama") {
        return true;
    }
    let words = heading.split(|character: char| !character.is_alphanumeric()).filter(|word| !word.is_empty()).collect::<Vec<_>>();
    words.contains(&"drama")
        && !words.iter().any(|word| {
            matches!(
                *word,
                "criticism"
                    | "interpretation"
                    | "history"
                    | "theory"
                    | "study"
                    | "studies"
                    | "bibliography"
                    | "dictionaries"
                    | "encyclopedias"
                    | "fiction"
                    | "poetry"
                    | "biography"
                    | "humor"
                    | "correspondence"
                    | "letters"
                    | "essays"
                    | "interviews"
                    | "speeches"
                    | "juvenile"
                    | "young"
            )
        })
}

/// Preserve the raw heading and source when materializing an approved mapping
/// for enrichment or a catalog backfill. Already derived subjects are skipped.
pub fn reviewed_source_heading_subject(subject: &BookSubject) -> Option<BookSubject> {
    if subject.authority() == Some(UNIFIED_SYSTEM_ID) {
        return None;
    }
    let path = reviewed_source_heading_path(subject)?;
    BookSubject::new(None, subject.name(), format!("{SOURCE_HEADING_PREFIX}{}", subject.source()), Some(UNIFIED_SYSTEM_ID.into()), Some(path.into())).ok()
}
