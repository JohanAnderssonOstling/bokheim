use std::fmt;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudiobookChapter {
    pub title: String,
    pub start: Duration,
    pub end: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudibleRegion {
    UnitedStates,
    Canada,
    UnitedKingdom,
    Australia,
    France,
    Germany,
    Japan,
    Italy,
    India,
    Spain,
}

impl AudibleRegion {
    pub const ALL: [Self; 10] = [Self::UnitedStates, Self::Canada, Self::UnitedKingdom, Self::Australia, Self::France, Self::Germany, Self::Japan, Self::Italy, Self::India, Self::Spain];

    pub fn code(self) -> &'static str {
        match self {
            Self::UnitedStates => "US",
            Self::Canada => "CA",
            Self::UnitedKingdom => "UK",
            Self::Australia => "AU",
            Self::France => "FR",
            Self::Germany => "DE",
            Self::Japan => "JP",
            Self::Italy => "IT",
            Self::India => "IN",
            Self::Spain => "ES",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataProviderId {
    GoogleBooks,
    OpenLibrary,
    AppleBooks,
    Audible(AudibleRegion),
    Audnexus,
    AudiobookCovers,
    FantLab,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataProviderDescriptor {
    pub id: MetadataProviderId,
    pub name: String,
    pub supplies_metadata: bool,
    pub supplies_covers: bool,
}

impl MetadataProviderDescriptor {
    pub fn custom(id: impl Into<String>, name: impl Into<String>, supplies_covers: bool) -> Self {
        Self { id: MetadataProviderId::Custom(id.into()), name: name.into(), supplies_metadata: true, supplies_covers }
    }
}

pub fn builtin_metadata_providers() -> Vec<MetadataProviderDescriptor> {
    let mut providers =
        vec![metadata_provider(MetadataProviderId::GoogleBooks, "Google Books", true), metadata_provider(MetadataProviderId::OpenLibrary, "Open Library", true), metadata_provider(MetadataProviderId::AppleBooks, "Apple Books", true)];
    providers.extend(AudibleRegion::ALL.into_iter().map(|region| MetadataProviderDescriptor { id: MetadataProviderId::Audible(region), name: format!("Audible {}", region.code()), supplies_metadata: true, supplies_covers: true }));
    providers.extend([metadata_provider(MetadataProviderId::Audnexus, "Audnexus", true), metadata_provider(MetadataProviderId::AudiobookCovers, "AudiobookCovers.com", true), metadata_provider(MetadataProviderId::FantLab, "FantLab", true)]);
    providers
}

fn metadata_provider(id: MetadataProviderId, name: &str, supplies_covers: bool) -> MetadataProviderDescriptor {
    MetadataProviderDescriptor { id, name: name.to_owned(), supplies_metadata: true, supplies_covers }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataLookup {
    pub title: String,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub duration: Option<Duration>,
}

impl MetadataLookup {
    pub fn from_embedded(title: impl Into<String>, author: Option<String>, narrator: Option<String>, duration: Duration) -> Self {
        Self { title: title.into(), author, narrator, duration: Some(duration), ..Self::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataMatch {
    pub provider: MetadataProviderId,
    pub provider_item_id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub duration: Option<Duration>,
    pub cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderError {
    pub message: String,
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {}

pub trait MetadataProvider: Send + Sync {
    fn descriptor(&self) -> &MetadataProviderDescriptor;
    fn search(&self, lookup: &MetadataLookup) -> Result<Vec<MetadataMatch>, ProviderError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterProviderId {
    EmbeddedM4b,
    Audnexus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterOrigin {
    EmbeddedM4b,
    Audnexus,
    FullAudiobookFallback,
}

impl ChapterOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::EmbeddedM4b => "Embedded M4B chapters",
            Self::Audnexus => "Audnexus chapters",
            Self::FullAudiobookFallback => "No embedded chapters",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterLookup {
    pub asin: String,
    pub region: AudibleRegion,
    pub expected_duration: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterSet {
    pub origin: ChapterOrigin,
    pub runtime: Duration,
    pub chapters: Vec<AudiobookChapter>,
}

pub trait ChapterProvider: Send + Sync {
    fn id(&self) -> ChapterProviderId;
    fn find_chapters(&self, lookup: &ChapterLookup) -> Result<Option<ChapterSet>, ProviderError>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChapterSelectionPolicy {
    pub automatic_runtime_delta: f64,
    pub review_runtime_delta: f64,
    pub minimum_boundary_alignment: f64,
    pub minimum_boundary_tolerance: Duration,
    pub boundary_tolerance_ratio: f64,
}

impl Default for ChapterSelectionPolicy {
    fn default() -> Self {
        Self { automatic_runtime_delta: 0.03, review_runtime_delta: 0.07, minimum_boundary_alignment: 0.80, minimum_boundary_tolerance: Duration::from_secs(90), boundary_tolerance_ratio: 0.01 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChapterDecision {
    KeepEmbeddedDescriptive,
    KeepEmbeddedNoExternalMatch,
    UseExternalNoEmbeddedChapters,
    UseExternalGenericEmbedded,
    ReviewRuntimeDifference,
    ReviewBoundaryDifference,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChapterResolution {
    pub selected: ChapterSet,
    pub external_candidate: Option<ChapterSet>,
    pub decision: ChapterDecision,
    pub runtime_delta: Option<f64>,
    pub boundary_alignment: Option<f64>,
}

pub fn resolve_chapters(embedded: ChapterSet, external: Option<ChapterSet>, audiobook_duration: Duration, policy: ChapterSelectionPolicy) -> ChapterResolution {
    let Some(external) = external else {
        return ChapterResolution { selected: embedded, external_candidate: None, decision: ChapterDecision::KeepEmbeddedNoExternalMatch, runtime_delta: None, boundary_alignment: None };
    };

    let runtime_delta = duration_delta_ratio(external.runtime, audiobook_duration);
    let embedded_is_fallback = embedded.origin == ChapterOrigin::FullAudiobookFallback;
    let embedded_is_generic = is_incrementing_number_pattern(&embedded.chapters);
    let external_is_generic = is_incrementing_number_pattern(&external.chapters);
    let alignment = boundary_alignment(&embedded.chapters, &external.chapters, audiobook_duration, policy);

    if !embedded_is_fallback && !embedded_is_generic {
        return resolution_with_candidate(embedded, external, ChapterDecision::KeepEmbeddedDescriptive, runtime_delta, alignment);
    }
    if runtime_delta > policy.review_runtime_delta {
        return resolution_with_candidate(embedded, external, ChapterDecision::ReviewRuntimeDifference, runtime_delta, alignment);
    }
    if !embedded_is_fallback && alignment < policy.minimum_boundary_alignment {
        return resolution_with_candidate(embedded, external, ChapterDecision::ReviewBoundaryDifference, runtime_delta, alignment);
    }
    if runtime_delta > policy.automatic_runtime_delta {
        return resolution_with_candidate(embedded, external, ChapterDecision::ReviewRuntimeDifference, runtime_delta, alignment);
    }
    if external_is_generic {
        return resolution_with_candidate(embedded, external, ChapterDecision::KeepEmbeddedNoExternalMatch, runtime_delta, alignment);
    }

    ChapterResolution {
        selected: external,
        external_candidate: None,
        decision: if embedded_is_fallback { ChapterDecision::UseExternalNoEmbeddedChapters } else { ChapterDecision::UseExternalGenericEmbedded },
        runtime_delta: Some(runtime_delta),
        boundary_alignment: Some(alignment),
    }
}

fn resolution_with_candidate(embedded: ChapterSet, external: ChapterSet, decision: ChapterDecision, runtime_delta: f64, alignment: f64) -> ChapterResolution {
    ChapterResolution { selected: embedded, external_candidate: Some(external), decision, runtime_delta: Some(runtime_delta), boundary_alignment: Some(alignment) }
}

fn duration_delta_ratio(left: Duration, right: Duration) -> f64 {
    let denominator = right.as_secs_f64().max(1.0);
    (left.as_secs_f64() - right.as_secs_f64()).abs() / denominator
}

fn boundary_alignment(embedded: &[AudiobookChapter], external: &[AudiobookChapter], duration: Duration, policy: ChapterSelectionPolicy) -> f64 {
    let embedded_boundaries = embedded.iter().skip(1).map(|chapter| chapter.start).collect::<Vec<_>>();
    let external_boundaries = external.iter().skip(1).map(|chapter| chapter.start).collect::<Vec<_>>();
    if embedded_boundaries.is_empty() {
        return 1.0;
    }
    if external_boundaries.is_empty() {
        return 0.0;
    }

    let tolerance = policy.minimum_boundary_tolerance.max(Duration::from_secs_f64(duration.as_secs_f64() * policy.boundary_tolerance_ratio));
    let matches = embedded_boundaries.iter().filter(|embedded_start| external_boundaries.iter().any(|external_start| duration_difference(**embedded_start, *external_start) <= tolerance)).count();
    matches as f64 / embedded_boundaries.len() as f64
}

fn duration_difference(left: Duration, right: Duration) -> Duration {
    if left >= right {
        left - right
    } else {
        right - left
    }
}

pub fn is_incrementing_number_pattern(chapters: &[AudiobookChapter]) -> bool {
    if chapters.len() < 2 {
        return false;
    }
    let Some((prefix, first)) = numbered_suffix(&chapters[0].title) else { return false };
    chapters.iter().enumerate().all(|(index, chapter)| numbered_suffix(&chapter.title).is_some_and(|(candidate_prefix, number)| candidate_prefix == prefix && number == first + index as u64))
}

fn numbered_suffix(title: &str) -> Option<(String, u64)> {
    let title = title.trim();
    let digit_start = title.char_indices().rev().take_while(|(_, character)| character.is_ascii_digit()).last()?.0;
    let prefix = title[..digit_start].trim_end_matches(|character: char| character.is_whitespace() || matches!(character, '-' | '_' | ':' | '.')).trim();
    if prefix.is_empty() {
        return None;
    }
    let number = title[digit_start..].parse().ok()?;
    Some((prefix.to_ascii_lowercase(), number))
}
