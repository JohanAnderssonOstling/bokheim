//! On-demand matching against the selected taxonomy, with a bounded SQLite page cache.
use crate::runtime::{MatchSource, UnifiedTaxonomyNavigation};
use crate::{ROOT_SUBJECT_PATH, UnifiedConceptDefinition, UnifiedConceptRoute, installed, matcher_storage, runtime_format};
use rusqlite::{Connection, OptionalExtension, Params, Row, params};
use std::collections::{BTreeMap, BTreeSet};
use subject_taxonomy::runtime_index::{LccSelector, build_runtime};

pub(super) struct SqliteTaxonomy {
    db: Connection,
    pub revision_id: String,
}

impl SqliteTaxonomy {
    pub fn from_connection(db: Connection) -> Result<Self, String> {
        db.pragma_update(None, "cache_size", -installed::PAGE_CACHE_KIB).map_err(|e| e.to_string())?;
        db.pragma_update(Some("temp"), "cache_size", -installed::PAGE_CACHE_KIB).map_err(|e| e.to_string())?;
        db.set_prepared_statement_cache_capacity(16);
        Self::validate(&db)?;
        let revision_id = db.query_row("SELECT value FROM taxonomy_meta WHERE key='client_revision'", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        Ok(Self { db, revision_id })
    }

    pub(super) fn validate(db: &Connection) -> Result<(), String> {
        runtime_format::validate(db)
    }

    pub fn from_concepts(concepts: Vec<UnifiedConceptDefinition>) -> Result<Self, String> {
        let runtime = build_runtime(concepts.clone())?;
        let db = Connection::open_in_memory().map_err(|e| e.to_string())?;
        db.execute_batch(
            "CREATE TABLE taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
             CREATE TABLE concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
             CREATE TABLE concept_parent(concept_id INTEGER,parent_concept_id INTEGER,ordinal INTEGER,PRIMARY KEY(concept_id,parent_concept_id)) WITHOUT ROWID;
             CREATE TABLE lcc_selector(concept_id INTEGER,selector TEXT);
             CREATE TABLE bisac_selector(concept_id INTEGER,selector TEXT);
             CREATE TABLE ddc_selector(concept_id INTEGER,selector TEXT);",
        )
        .map_err(|e| e.to_string())?;
        db.execute("INSERT INTO taxonomy_meta VALUES('format_version',?1),('minimum_projection_version',?2)", params![runtime_format::FORMAT_VERSION.to_string(), runtime_format::PROJECTION_VERSION.to_string()])
            .map_err(|e| e.to_string())?;
        // Owned authoring matchers use the selected runtime's reviewed policy data too.
        let policy = installed::read_only_uri(&installed::path().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        db.execute("ATTACH ?1 AS runtime_policy", [policy]).map_err(|e| e.to_string())?;
        db.execute_batch(
            "CREATE TABLE taxonomy_path_alias(path TEXT PRIMARY KEY,concept_id INTEGER NOT NULL) WITHOUT ROWID;
             INSERT INTO taxonomy_path_alias SELECT path,concept_id FROM runtime_policy.taxonomy_path_alias;
             CREATE TABLE taxonomy_policy(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID;
             INSERT INTO taxonomy_policy SELECT key,value FROM runtime_policy.taxonomy_policy;
             DETACH runtime_policy;",
        )
        .map_err(|e| e.to_string())?;
        db.execute_batch(include_str!("range_storage.sql")).map_err(|e| e.to_string())?;
        for concept in concepts {
            db.execute("INSERT INTO concept VALUES(?1,?2)", params![concept.concept_id, concept.preferred_label]).map_err(|e| e.to_string())?;
            for (position, parent) in concept.parent_ids.iter().enumerate() {
                db.execute("INSERT INTO concept_parent VALUES(?1,?2,?3)", params![concept.concept_id, parent, position as i64]).map_err(|e| e.to_string())?;
            }
            for (system, selectors) in concept.source_selectors {
                for selector in selectors {
                    if system == "lcc" && selector.contains("..") {
                        let (start, end) = selector.split_once("..").unwrap();
                        subject_taxonomy::range_storage::insert_range(&db, concept.concept_id, start, end)?;
                    } else {
                        let table = match system.as_str() {
                            "lcc" => "lcc_selector",
                            "bisac" => "bisac_selector",
                            "ddc" => "ddc_selector",
                            _ => continue,
                        };
                        db.execute(&format!("INSERT INTO {table} VALUES(?1,?2)"), params![concept.concept_id, selector]).map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        db.execute_batch(include_str!("route_storage.sql")).map_err(|e| e.to_string())?;
        matcher_storage::prepare(&db, &runtime)?;
        Self::from_connection(db)
    }

    fn rows<T>(&self, sql: &str, parameters: impl Params, row: impl FnMut(&Row<'_>) -> rusqlite::Result<T>) -> Vec<T> {
        self.db.prepare_cached(sql).expect("prepare taxonomy lookup").query_map(parameters, row).expect("query taxonomy lookup").collect::<rusqlite::Result<Vec<_>>>().expect("read taxonomy lookup")
    }

    pub fn find_path(&self, path: &str) -> Option<i64> {
        self.rows(
            "WITH RECURSIVE paths(parent_route_id,path,concept_id) AS (
                SELECT r.parent_route_id,c.preferred_label,r.concept_id
                FROM taxonomy_match_route r JOIN concept c USING(concept_id) WHERE r.path_hash=?2
                UNION ALL
                SELECT r.parent_route_id,c.preferred_label||' / '||p.path,p.concept_id
                FROM paths p JOIN taxonomy_match_route r ON r.route_id=p.parent_route_id JOIN concept c ON c.concept_id=r.concept_id
             ) SELECT concept_id FROM paths WHERE parent_route_id IS NULL AND path=?1 LIMIT 1",
            params![path, matcher_storage::path_hash(path)],
            |r| r.get(0),
        )
        .into_iter()
        .next()
    }

    pub fn concept_for_path(&self, path: &str) -> Option<i64> {
        if !path.starts_with("concept:") {
            if let Some(id) = self.find_path(path) {
                return Some(id);
            }
        }
        if let Some(id) = path.strip_prefix("concept:").and_then(|id| id.parse::<i64>().ok()).filter(|id| self.contains_concept(*id)) {
            return Some(id);
        }
        self.reference_identity(path).filter(|id| self.contains_concept(*id))
    }

    pub fn reference_identity(&self, path: &str) -> Option<i64> {
        let exists = |id| self.db.prepare_cached("SELECT EXISTS(SELECT 1 FROM concept WHERE concept_id=?1)").expect("prepare taxonomy definition").query_row([id], |r| r.get(0)).expect("read taxonomy definition");
        if let Some(id) = path.strip_prefix("concept:").and_then(|id| id.parse::<i64>().ok()).filter(|id| exists(*id)) {
            return Some(id);
        }
        self.db.prepare_cached("SELECT concept_id FROM taxonomy_path_alias WHERE path=?1").expect("prepare taxonomy redirect").query_row([path], |r| r.get::<_, i64>(0)).optional().expect("read taxonomy redirect").filter(|id| exists(*id))
    }

    pub fn policy(&self, key: &str) -> String {
        self.db.prepare_cached("SELECT value FROM taxonomy_policy WHERE key=?1").expect("prepare taxonomy policy").query_row([key], |r| r.get(0)).expect("read taxonomy policy")
    }

    pub fn contains_concept(&self, concept_id: i64) -> bool {
        self.db.prepare_cached("SELECT EXISTS(SELECT 1 FROM taxonomy_match_route WHERE concept_id=?1)").expect("prepare taxonomy identity lookup").query_row([concept_id], |row| row.get(0)).expect("query taxonomy identity")
    }

    pub fn contains_usable_concept(&self, concept_id: i64) -> bool {
        self.db
            .prepare_cached("SELECT EXISTS(SELECT 1 FROM taxonomy_match_route r JOIN concept c USING(concept_id) WHERE r.concept_id=?1 AND (r.parent_route_id IS NOT NULL OR c.preferred_label!=?2))")
            .expect("prepare usable taxonomy identity lookup")
            .query_row(params![concept_id, ROOT_SUBJECT_PATH], |row| row.get(0))
            .expect("query usable taxonomy identity")
    }

    pub fn concept_routes(&self, concepts: &BTreeSet<i64>) -> Vec<Vec<i64>> {
        if concepts.is_empty() {
            return Vec::new();
        }
        let mut statement = self
            .db
            .prepare_cached(
                "WITH RECURSIVE ancestors(origin,concept_id,parent_route_id,distance) AS (
                SELECT r.route_id,r.concept_id,r.parent_route_id,0
                FROM json_each(?1) ids JOIN taxonomy_match_route r ON r.concept_id=ids.value
                UNION ALL
                SELECT a.origin,r.concept_id,r.parent_route_id,a.distance+1
                FROM ancestors a JOIN taxonomy_match_route r ON r.route_id=a.parent_route_id
             ) SELECT origin,concept_id FROM ancestors ORDER BY origin,distance DESC",
            )
            .expect("prepare taxonomy route identities");
        let rows = statement.query_map([serde_json::to_string(concepts).expect("serialize taxonomy IDs")], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))).expect("query taxonomy route identities");
        let mut routes: Vec<Vec<i64>> = Vec::new();
        let mut previous = None;
        for row in rows {
            let (origin, concept_id) = row.expect("read taxonomy route identities");
            if previous != Some(origin) {
                routes.push(Vec::new());
                previous = Some(origin);
            }
            routes.last_mut().expect("route identity group").push(concept_id);
        }
        routes
    }

    pub fn navigation(&self) -> UnifiedTaxonomyNavigation {
        let rows = self.rows(
            "SELECT DISTINCT child.concept_id,parent.concept_id
             FROM taxonomy_match_route child LEFT JOIN taxonomy_match_route parent ON parent.route_id=child.parent_route_id",
            [],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        );
        let mut graph = UnifiedTaxonomyNavigation { revision_id: self.revision_id.clone(), parents: BTreeMap::new(), roots: BTreeSet::new() };
        for (child, parent) in rows {
            let parents = graph.parents.entry(child).or_default();
            if let Some(parent) = parent {
                parents.insert(parent);
            } else {
                graph.roots.insert(child);
            }
        }
        graph
    }

    pub fn paths_for_concept(&self, concept: i64) -> Vec<String> {
        self.rows(
            "WITH RECURSIVE paths(parent_route_id,path) AS (
                SELECT r.parent_route_id,c.preferred_label FROM taxonomy_match_route r JOIN concept c USING(concept_id) WHERE r.concept_id=?1
                UNION ALL
                SELECT r.parent_route_id,c.preferred_label||' / '||p.path
                FROM paths p JOIN taxonomy_match_route r ON r.route_id=p.parent_route_id JOIN concept c ON c.concept_id=r.concept_id
             ) SELECT path FROM paths WHERE parent_route_id IS NULL ORDER BY path",
            [concept],
            |r| r.get(0),
        )
    }

    pub fn routes(&self) -> Vec<UnifiedConceptRoute> {
        self.rows(
            "WITH RECURSIVE paths(route_id,concept_id,path,parent_path,label) AS (
                SELECT r.route_id,r.concept_id,c.preferred_label,?1,c.preferred_label
                FROM taxonomy_match_route r JOIN concept c USING(concept_id) WHERE r.parent_route_id IS NULL
                UNION ALL
                SELECT r.route_id,r.concept_id,p.path||' / '||c.preferred_label,p.path,c.preferred_label
                FROM paths p JOIN taxonomy_match_route r ON r.parent_route_id=p.route_id JOIN concept c ON c.concept_id=r.concept_id
             ) SELECT p.concept_id,p.path,p.parent_path,p.label,o.sort_order FROM paths p JOIN taxonomy_match_order o USING(concept_id) ORDER BY p.path",
            [ROOT_SUBJECT_PATH],
            |r| Ok(UnifiedConceptRoute { concept_id: r.get(0)?, path: r.get(1)?, parent_path: r.get(2)?, label: r.get(3)?, sort_order: r.get(4)? }),
        )
    }
}

impl MatchSource for SqliteTaxonomy {
    fn exact(&self, system: &str, code: &str) -> Vec<i64> {
        self.rows("SELECT concept_id FROM taxonomy_match_exact WHERE system_id=?1 AND code=?2 ORDER BY concept_id", params![system, code], |r| r.get(0))
    }
    fn visit_prefixes(&self, system: &str, mut visit: impl FnMut(&str, i64)) {
        let mut statement = self.db.prepare_cached("SELECT prefix,concept_id FROM taxonomy_match_prefix WHERE system_id=?1").expect("prepare taxonomy prefixes");
        let mut rows = statement.query([system]).expect("query taxonomy prefixes");
        while let Some(row) = rows.next().expect("read taxonomy prefix") {
            // Borrow SQLite's text for this row; only matching IDs are retained.
            let prefix = row.get_ref(0).expect("read taxonomy prefix text");
            visit(prefix.as_str().expect("taxonomy prefix is text"), row.get(1).expect("read taxonomy prefix concept"));
        }
    }
    fn ranges(&self, letters: &str, number: f64) -> Vec<LccSelector> {
        self.rows(
            "SELECT r.concept_id,r.start_letters,r.start_number,r.start_cutters,r.end_letters,r.end_number,r.end_cutters
             FROM json_each((SELECT range_ids FROM taxonomy_match_interval
                 WHERE letters=?1 AND (number,after_endpoint)<=(?2,0) ORDER BY number DESC,after_endpoint DESC LIMIT 1)) ids
             JOIN taxonomy_match_range r ON r.range_id=ids.value",
            params![letters, number],
            |r| {
                let parse = |column| {
                    let text: String = r.get(column)?;
                    serde_json::from_str(&text).map_err(|e| rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(e)))
                };
                Ok(LccSelector { concept_id: r.get(0)?, start_letters: r.get(1)?, start_number: r.get(2)?, start_cutters: parse(3)?, end_letters: r.get(4)?, end_number: r.get(5)?, end_cutters: parse(6)? })
            },
        )
    }
    fn has_subclass(&self, letters: &str) -> bool {
        !self.rows("SELECT 1 FROM taxonomy_match_subclass WHERE letters=?1", [letters], |r| r.get::<_, i64>(0)).is_empty()
    }
    fn assigned_ancestors(&self, concepts: &BTreeSet<i64>) -> BTreeSet<i64> {
        if concepts.is_empty() {
            return BTreeSet::new();
        }
        self.rows(
            "SELECT DISTINCT ancestor.value FROM json_each(?1) ids
             JOIN taxonomy_match_order c ON c.concept_id=ids.value JOIN json_each(c.ancestors) ancestor",
            [serde_json::to_string(concepts).expect("serialize taxonomy IDs")],
            |r| r.get(0),
        )
        .into_iter()
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn path_hash_collisions_still_require_exact_path_equality() {
        let definitions = vec![
            UnifiedConceptDefinition::new(1, "Root".into(), vec![], BTreeMap::new()),
            UnifiedConceptDefinition::new(2, "Leaf / literal separator".into(), vec![1], BTreeMap::new()),
            UnifiedConceptDefinition::new(3, "Other".into(), vec![], BTreeMap::from([("lcc".into(), vec!["QA20".into()])])),
        ];
        let taxonomy = SqliteTaxonomy::from_concepts(definitions).unwrap();
        for (path, expected) in [("Root", Some(1)), ("Root / Leaf / literal separator", Some(2)), ("Other", Some(3)), ("Missing", None), ("Root / ", None)] {
            taxonomy.db.execute("UPDATE taxonomy_match_route SET path_hash=?1", [matcher_storage::path_hash(path)]).unwrap();
            assert_eq!(taxonomy.find_path(path), expected, "{path}");
        }
    }

    #[test]
    fn matching_requires_the_current_prepared_format() {
        for invalidate in ["UPDATE taxonomy_meta SET value='0' WHERE key='client_matcher_version'", "DROP TABLE taxonomy_match_exact", "DROP TABLE taxonomy_match_route"] {
            let definitions = vec![UnifiedConceptDefinition::new(1, "Stored".into(), vec![], BTreeMap::from([("lcc".into(), vec!["QA20".into()])]))];
            let taxonomy = SqliteTaxonomy::from_concepts(definitions).unwrap();
            taxonomy.db.execute_batch(invalidate).unwrap();
            let result = SqliteTaxonomy::from_connection(taxonomy.db);
            assert!(matches!(result, Err(error) if error.contains("must be prepared")));
        }
    }
}
