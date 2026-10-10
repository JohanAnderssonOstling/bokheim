use crate::*;
use metadata_contract::matching::{author_match_keys, bibliographic_match_key, matching_author_count, title_qualifiers, NormalizedTitle};
pub(crate) fn rank_matches(query: &EditionIdentityQuery, author_keys: &BTreeSet<String>, mut editions: Vec<(usize, usize, usize, EditionIdentityMatch)>) -> EditionIdentityResult {
    if editions.len() > 1 {
        let best_specificity = editions.iter().map(|(specificity, _, _, _)| *specificity).max().unwrap_or(0);
        editions.retain(|(specificity, _, _, _)| *specificity == best_specificity);
    }
    if editions.len() > 1 {
        let best_authors = editions.iter().map(|(_, count, _, _)| *count).max().unwrap_or(0);
        editions.retain(|(_, count, _, _)| *count == best_authors);
    }
    if editions.len() > 1 && !author_keys.is_empty() {
        let best_score = editions.iter().map(|(_, _, score, _)| *score).max().unwrap_or(0);
        if best_score > 0 {
            editions.retain(|(_, _, score, _)| *score == best_score);
        }
    }
    if editions.len() > 1 {
        let mut seen_works = BTreeSet::new();
        editions.retain(|(_, _, _, matched)| matched.open_library_work_id.as_ref().is_none_or(|work_id| seen_works.insert(work_id.clone())));
    }
    // Duplicate records sharing one ISBN and identical codes can collapse.
    // Different ISBNs remain unresolved even when their classifications agree;
    // callers can recover work subjects without inventing an edition identity.
    if editions.len() > 1 && editions.iter().all(|(_, matched_author, _, _)| *matched_author > 0) {
        let classification_codes = |matched: &EditionIdentityMatch| {
            matched.classifications.iter().map(|classification| (classification.scheme, classification.notation.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_uppercase).collect::<String>())).collect::<BTreeSet<_>>()
        };
        let first_codes = classification_codes(&editions[0].3);
        if !first_codes.is_empty()
            && editions.iter().skip(1).all(|(_, _, _, matched)| matched.canonical_isbn13 == editions[0].3.canonical_isbn13 && matched.book_year == editions[0].3.book_year && classification_codes(matched) == first_codes)
        {
            editions.truncate(1);
        }
    }
    // Title agreement identifies candidates, but it is not enough to attach
    // an external edition automatically: generic titles such as "Complete
    // Works" routinely collide across unrelated authors. Publisher and year
    // may rank editions of the same work, while at least one author must agree
    // before the remaining candidate is safe to accept without review.
    let automatic_match = editions.len() == 1 && editions[0].1 > 0;
    let mut strong = editions.into_iter().map(|(_, _, _, matched)| matched).collect::<Vec<_>>();
    match strong.len() {
        0 => identity_result(&query.query_id, EditionIdentityStatus::NoMatch, None, Vec::new(), false),
        1 if automatic_match => identity_result(&query.query_id, EditionIdentityStatus::Matched, strong.pop(), Vec::new(), false),
        _ => {
            let candidates_truncated = strong.len() > MAX_EDITION_IDENTITY_REVIEW_CANDIDATES;
            strong.truncate(MAX_EDITION_IDENTITY_REVIEW_CANDIDATES);
            identity_result(&query.query_id, EditionIdentityStatus::Ambiguous, None, strong, candidates_truncated)
        }
    }
}

pub(crate) fn resolve(connection: &Connection, queries: Vec<EditionIdentityQuery>) -> Result<Vec<EditionIdentityResult>, MetadataError> {
    queries.into_iter().map(|query| {
        if query.query_id.is_empty() || query.query_id.len()>256 || query.title.len()>4096 || query.authors.len()>32 || query.publishers.len()>32 {
            return Err(MetadataError("edition query contains too many or oversized fields".into()));
        }
        let title = NormalizedTitle::new(&query.title);
        let author_keys = query.authors.iter().flat_map(|a|author_match_keys(a)).collect::<BTreeSet<_>>();
        let publisher_keys = query.publishers.iter().map(|p|bibliographic_match_key(p)).collect::<BTreeSet<_>>();
        let mut result = identity_result(&query.query_id, if title.full.is_empty(){EditionIdentityStatus::InsufficientMetadata}else{EditionIdentityStatus::NoMatch},None,vec![],false);
        let mut stmt = connection.prepare_cached("SELECT c.isbn13,c.edition_id,c.work_id,c.title,c.subtitle,c.work_title,c.authors,c.publishers,c.book_year,c.classifications,k.edition_match,k.work_match FROM edition_title_key k JOIN edition_candidate c USING(edition_id) WHERE k.key=?1 ORDER BY k.edition_id LIMIT 4097").map_err(error)?;
        for (key,specificity) in title.candidate_keys(MAX_TITLE_SUBSTRINGS_PER_QUERY) {
            let rows=stmt.query_map([key],|r|Ok((r.get::<_,crate::Isbn13>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,String>(3)?,r.get::<_,Option<String>>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,Option<i32>>(8)?,r.get::<_,Vec<u8>>(9)?,r.get::<_,bool>(10)?,r.get::<_,bool>(11)?))).map_err(error)?.collect::<Result<Vec<_>,_>>().map_err(error)?;
            if rows.len()>MAX_REVERSE_TITLE_CANDIDATES { return Ok(identity_result(&query.query_id,EditionIdentityStatus::Ambiguous,None,vec![],true)); }
            let mut matches=vec![];
            for (isbn,id,work,title,subtitle,work_title,authors,publishers,year,codes,edition_match,work_match) in rows {
                if title_qualifiers(&title)!=title_qualifiers(&query.title){continue;}
                let authors:Vec<String>=serde_json::from_str(&authors).map_err(error)?;
                let count=matching_author_count(&query.authors,&authors);
                if !author_keys.is_empty() && count==0{continue;}
                let publishers:Vec<String>=serde_json::from_str(&publishers).map_err(error)?;
                let matched_publisher=publishers.iter().any(|p|publisher_keys.contains(&bibliographic_match_key(p)));
                let matched_book_year=query.book_year.is_some() && query.book_year==year;
                let m=EditionIdentityMatch{canonical_isbn13:isbn.to_string(),open_library_edition_id:if id>0{format!("OL{id}M")}else{String::new()},open_library_work_id:work.map(|w|format!("OL{w}W")),title,subtitle,work_title,authors,publishers,book_year:year,title_match:match (edition_match,work_match){(true,true)=>EditionTitleMatch::Both,(true,false)=>EditionTitleMatch::Edition,_=>EditionTitleMatch::Work},matched_publisher,matched_book_year,classifications:serde_json::from_slice(&codes).map_err(error)?};
                matches.push((specificity,count,usize::from(matched_publisher)+usize::from(matched_book_year),m));
            }
            result=rank_matches(&query,&author_keys,matches);
            if result.status!=EditionIdentityStatus::NoMatch{break;}
        }
        if result.status==EditionIdentityStatus::NoMatch { result.authority_subjects=authority(connection,&query)?; }
        Ok(result)
    }).collect()
}

fn identity_result(id: &str, status: EditionIdentityStatus, matched: Option<EditionIdentityMatch>, candidates: Vec<EditionIdentityMatch>, candidates_truncated: bool) -> EditionIdentityResult {
    EditionIdentityResult { query_id: id.into(), status, matched, candidates, candidates_truncated, authority_subjects: None }
}

fn authority(connection: &Connection, query: &EditionIdentityQuery) -> Result<Option<metadata_contract::AuthoritySubjectMatch>, MetadataError> {
    use metadata_contract::AuthoritySubjectMatch;
    if query.authors.is_empty() {
        return Ok(None);
    }
    let normalized = NormalizedTitle::new(&query.title);
    let mut stmt = connection.prepare_cached("SELECT codes FROM authority_title WHERE key=?1 AND source=?2 ORDER BY control_number LIMIT 129").map_err(error)?;
    for provider in ["library_of_congress", "wikidata"] {
        let mut groups = vec![];
        let mut wikidata = BTreeMap::new();
        for key in normalized.lookup_keys() {
            let rows = stmt.query_map(params![key, provider], |r| r.get::<_, Vec<u8>>(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            if rows.len() > 128 {
                return Ok(None);
            }
            let records = rows.into_iter().map(|raw| serde_json::from_slice::<AuthoritySubjectMatch>(&raw).map_err(error)).collect::<Result<Vec<_>, _>>()?;
            if provider == "wikidata" {
                for m in records {
                    let id = m.record_ids[0].strip_prefix('Q').and_then(|id| id.parse::<i64>().ok()).ok_or_else(|| MetadataError("invalid authority control number".into()))?;
                    wikidata.insert(-id, m);
                }
            } else {
                groups.push(records);
            }
        }
        if provider == "wikidata" {
            if wikidata.len() > 128 {
                return Ok(None);
            }
            groups.push(wikidata.into_values().collect());
        }
        for records in groups {
            let mut accepted: Vec<AuthoritySubjectMatch> = vec![];
            let mut best = 0;
            for m in records {
                let record = NormalizedTitle::new(&m.title);
                if normalized.qualifiers != record.qualifiers {
                    continue;
                }
                if provider == "library_of_congress" {
                    let main = NormalizedTitle::new(&m.main_title).full;
                    if normalized.full != record.full && normalized.full != normalized.main && record.full != main {
                        continue;
                    }
                } else if !normalized.matches(&record) && !normalized.matches(&NormalizedTitle::new(&m.main_title)) {
                    continue;
                }
                let count = matching_author_count(&query.authors, &m.authors);
                if count == 0 || count < best {
                    continue;
                }
                if count > best {
                    accepted.clear();
                    best = count;
                }
                accepted.push(m);
            }
            if accepted.is_empty() {
                continue;
            }
            let mut result = accepted[0].clone();
            result.record_ids = accepted.iter().flat_map(|m| m.record_ids.clone()).collect();
            let mut codes = BTreeMap::<(ClassificationScheme, String), Classification>::new();
            for m in accepted {
                for c in m.classifications {
                    codes.entry((c.scheme, c.notation.clone())).and_modify(|old| old.evidence.extend(c.evidence.clone())).or_insert(c);
                }
            }
            result.classifications = codes.into_values().collect();
            return Ok(Some(result));
        }
    }
    Ok(None)
}
