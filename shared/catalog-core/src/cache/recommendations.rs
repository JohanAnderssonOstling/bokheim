use super::subject_browse::{language_supported, subject_distance, valid_subject_key};
use super::{
    decode_cached, normalize_search_text, params, CatalogCache, CatalogCacheError, CatalogPublication, CatalogRecommendation, CatalogRecommendationRequest, CatalogRecommendationResponse, CatalogSourceId, CatalogSubjectPreference, HashMap,
    HashSet,
};

impl CatalogCache {
    pub fn recommendations(&self, request: &CatalogRecommendationRequest) -> Result<CatalogRecommendationResponse, CatalogCacheError> {
        const MAX_PROFILE_SUBJECTS: usize = 64;
        const MAX_EXCLUSIONS: usize = 10_000;
        const MAX_RESULTS: usize = 100;
        if request.subjects.len() > MAX_PROFILE_SUBJECTS || request.excluded_publications.len() > MAX_EXCLUSIONS {
            return Err(CatalogCacheError::new("catalog recommendation request exceeds its bounded input size"));
        }
        let limit = usize::try_from(request.limit).unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS);
        let excluded = request.excluded_publications.iter().map(|item| (item.source_id.as_str().to_owned(), item.publication.edition_id.clone(), item.publication.work_id.clone().unwrap_or_default())).collect::<HashSet<_>>();
        let connection = self.pool.get()?;
        let mut candidates = HashMap::<(String, String, String), (CatalogSourceId, CatalogPublication, u64, HashMap<(String, String), u32>, i64, i64)>::new();
        let mut statement = connection.prepare(
            "SELECT p.source_id,p.edition_id,p.work_id,p.payload,a.subject_path,
                    COALESCE(pop.reading_log_count,0),COALESCE(pop.rating_count,0)
             FROM catalog_publication_subject_assignment a
             JOIN catalog_source s ON s.source_id=a.source_id AND s.active_generation=a.generation
             JOIN catalog_publication p ON p.source_id=a.source_id AND p.generation=a.generation AND p.edition_id=a.edition_id AND p.work_id=a.work_id
             LEFT JOIN catalog_publication_popularity pop
               ON pop.source_id=p.source_id AND pop.generation=p.generation AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
             WHERE a.system_id=?1 AND p.importable=1
               AND (a.subject_path=?2 OR instr(a.subject_path,?2 || ' / ')=1 OR instr(?2,a.subject_path || ' / ')=1)",
        )?;
        for preference in request.subjects.iter().filter(|item| item.weight > 0 && valid_subject_key(&item.system_id, &item.subject_path)) {
            let rows = statement.query_map(params![preference.system_id.to_ascii_lowercase(), preference.subject_path], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?))
            })?;
            for row in rows {
                let (source, edition, work, payload, assigned_path, reading_log_count, rating_count) = row?;
                if excluded.contains(&(source.clone(), edition.clone(), work.clone())) {
                    continue;
                }
                let publication: CatalogPublication = decode_cached(&payload)?;
                if !request.supported_languages.is_empty() && !publication.languages.is_empty() && !publication.languages.iter().any(|language| language_supported(language, &request.supported_languages)) {
                    continue;
                }
                let distance = subject_distance(&preference.subject_path, &assigned_path);
                let score = u64::from(preference.weight).saturating_mul(100) / u64::try_from(distance + 1).unwrap_or(u64::MAX);
                let key = (source.clone(), edition, work);
                let source_id = CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?;
                let entry = candidates.entry(key).or_insert_with(|| (source_id, publication, 0, HashMap::new(), reading_log_count, rating_count));
                entry.2 = entry.2.saturating_add(score);
                entry.3.entry((preference.system_id.to_ascii_lowercase(), assigned_path)).and_modify(|weight| *weight = (*weight).max(preference.weight)).or_insert(preference.weight);
            }
        }
        let mut recommendations = candidates
            .into_values()
            .map(|(source_id, publication, score, matches, reading_log_count, rating_count)| {
                (
                    CatalogRecommendation { source_id, publication, score, matched_subjects: matches.into_iter().map(|((system_id, subject_path), weight)| CatalogSubjectPreference { system_id, subject_path, weight }).collect() },
                    reading_log_count,
                    rating_count,
                )
            })
            .collect::<Vec<_>>();
        recommendations.sort_by(|left, right| {
            right
                .0
                .score
                .cmp(&left.0.score)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.publication.title.cmp(&right.0.publication.title))
                .then_with(|| left.0.source_id.as_str().cmp(right.0.source_id.as_str()))
        });
        let mut recommendations = recommendations.into_iter().map(|(recommendation, _, _)| recommendation).collect::<Vec<_>>();
        let mut seen_works = HashSet::new();
        recommendations.retain(|recommendation| seen_works.insert(recommendation_dedupe_key(recommendation)));
        recommendations.truncate(limit);
        Ok(CatalogRecommendationResponse { recommendations })
    }
}

pub(crate) fn recommendation_dedupe_key(recommendation: &CatalogRecommendation) -> String {
    let title = normalize_search_text(&recommendation.publication.title);
    let author = recommendation.publication.authors.first().map(|author| normalize_search_text(&author.name)).unwrap_or_default();
    if !title.is_empty() && !author.is_empty() {
        format!("bibliographic\n{title}\n{author}")
    } else {
        format!("identity\n{}\n{}\n{}", recommendation.source_id.as_str(), recommendation.publication.identity.edition_id, recommendation.publication.identity.work_id.as_deref().unwrap_or(""))
    }
}
