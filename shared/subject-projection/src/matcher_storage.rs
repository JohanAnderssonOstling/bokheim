//! Prepare indexed SQLite lookups once, when packaging or installing a taxonomy.
//! This module also runs in build.rs; clients never deserialize the compiled matcher.
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, BTreeSet};
use subject_taxonomy::lcc::compare_lcc_key;
use subject_taxonomy::runtime_index::{SelectorIndex, UnifiedTaxonomyRuntime};

pub const UNIFIED_MATCHER_VERSION: i64 = 151;

pub fn prepare(db: &Connection, runtime: &UnifiedTaxonomyRuntime) -> Result<(), String> {
    // Installation must not fsync once per selector. Reuse a caller's existing
    // transaction, otherwise make the complete preparation atomic here.
    let transaction = if db.is_autocommit() { Some(db.unchecked_transaction().map_err(|e| e.to_string())?) } else { None };
    db.execute_batch(
        "DROP TABLE IF EXISTS taxonomy_match_exact; DROP TABLE IF EXISTS taxonomy_match_prefix; DROP TABLE IF EXISTS taxonomy_match_subclass; DROP TABLE IF EXISTS taxonomy_match_order; DROP TABLE IF EXISTS taxonomy_match_range; DROP TABLE IF EXISTS taxonomy_match_interval; DROP TABLE IF EXISTS taxonomy_match_route;
         CREATE TABLE taxonomy_match_exact(system_id TEXT NOT NULL,code TEXT NOT NULL,concept_id INTEGER NOT NULL,PRIMARY KEY(system_id,code,concept_id)) WITHOUT ROWID;
         CREATE TABLE taxonomy_match_prefix(system_id TEXT NOT NULL,prefix TEXT NOT NULL,concept_id INTEGER NOT NULL,PRIMARY KEY(system_id,prefix,concept_id)) WITHOUT ROWID;
         CREATE TABLE taxonomy_match_subclass(letters TEXT PRIMARY KEY) WITHOUT ROWID;
         CREATE TABLE taxonomy_match_order(concept_id INTEGER PRIMARY KEY,sort_order INTEGER NOT NULL,ancestors TEXT NOT NULL);
         CREATE TABLE taxonomy_match_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER,path_hash INTEGER NOT NULL);
         CREATE INDEX taxonomy_match_route_hash ON taxonomy_match_route(path_hash);
         CREATE INDEX taxonomy_match_route_concept ON taxonomy_match_route(concept_id);
         CREATE TABLE taxonomy_match_range(range_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,start_letters TEXT NOT NULL,start_number REAL NOT NULL,start_cutters TEXT NOT NULL,end_letters TEXT NOT NULL,end_number REAL NOT NULL,end_cutters TEXT NOT NULL);
         CREATE TABLE taxonomy_match_interval(letters TEXT NOT NULL,number REAL NOT NULL,after_endpoint INTEGER NOT NULL,range_ids TEXT NOT NULL,PRIMARY KEY(letters,number,after_endpoint)) WITHOUT ROWID;",
    )
    .map_err(|e| e.to_string())?;
    let mut exact = db.prepare("INSERT INTO taxonomy_match_exact VALUES(?1,?2,?3)").map_err(|e| e.to_string())?;
    let mut prefix = db.prepare("INSERT INTO taxonomy_match_prefix VALUES(?1,?2,?3)").map_err(|e| e.to_string())?;
    let mut subclass = db.prepare("INSERT INTO taxonomy_match_subclass VALUES(?1)").map_err(|e| e.to_string())?;
    for (system, index) in runtime.selectors.iter() {
        for (code, ids) in &index.exact {
            for id in ids {
                exact.execute(params![system, code, id]).map_err(|e| e.to_string())?;
            }
        }
        for (code, id) in &index.prefixes {
            prefix.execute(params![system, code, id]).map_err(|e| e.to_string())?;
        }
        if system == "lcc" {
            for letters in &index.lcc_subclasses {
                subclass.execute([letters]).map_err(|e| e.to_string())?;
            }
            prepare_intervals(db, index)?;
        }
    }
    // Ancestry belongs to the complete concept graph, including concepts whose
    // display paths collide with another concept's canonical navigation route.
    let mut order = db.prepare("INSERT INTO taxonomy_match_order VALUES(?1,?2,?3)").map_err(|e| e.to_string())?;
    for (id, ancestors) in runtime.ancestors.iter() {
        order.execute(params![id, i64::MAX, serde_json::to_string(ancestors).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    }
    // Compile the matcher's path ownership, rather than the raw graph's route
    // occurrences. Store compact nodes and hash buckets; matching verifies full
    // reconstructed paths, so a hash collision never changes path ownership.
    let mut route_insert = db.prepare("INSERT INTO taxonomy_match_route VALUES(?1,?2,?3,?4)").map_err(|e| e.to_string())?;
    let mut sort_order = db.prepare("UPDATE taxonomy_match_order SET sort_order=?2 WHERE concept_id=?1").map_err(|e| e.to_string())?;
    for (id, route) in runtime.routes.values().enumerate() {
        let parent = runtime.routes.parent(id).map(|parent| parent as i64);
        route_insert.execute(params![id as i64, route.concept_id, parent, path_hash(&route.path)]).map_err(|e| e.to_string())?;
        sort_order.execute(params![route.concept_id, route.sort_order]).map_err(|e| e.to_string())?;
    }
    db.execute("INSERT INTO taxonomy_meta(key,value) VALUES('client_revision',?1),('client_matcher_version',?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![runtime.revision_id, UNIFIED_MATCHER_VERSION.to_string()])
        .map_err(|e| e.to_string())?;
    if let Some(transaction) = transaction {
        transaction.commit().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Stable across build scripts, native clients and Wasm. This selects candidates
/// only; callers must compare the complete path before accepting a match.
pub fn path_hash(path: &str) -> i64 {
    path.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)) as i64
}

fn prepare_intervals(db: &Connection, index: &SelectorIndex) -> Result<(), String> {
    // Each subclass has disjoint numeric intervals whose values are the small
    // set of potentially matching ranges. A lookup needs one B-tree seek,
    // rather than scanning every range in that subclass.
    let mut events = index.lcc_subclasses.iter().map(|letters| (letters.clone(), Vec::<(f64, bool, i64)>::new())).collect::<BTreeMap<_, _>>();
    let mut insert = db.prepare("INSERT INTO taxonomy_match_range VALUES(?1,?2,?3,?4,?5,?6,?7,?8)").map_err(|e| e.to_string())?;
    for (id, range) in index.lcc_ranges.iter().enumerate() {
        if compare_lcc_key(&range.start_letters, range.start_number, &range.end_letters, range.end_number).is_gt() {
            continue;
        }
        let id = id as i64;
        insert
            .execute(params![
                id,
                range.concept_id,
                range.start_letters,
                range.start_number,
                serde_json::to_string(&range.start_cutters).map_err(|e| e.to_string())?,
                range.end_letters,
                range.end_number,
                serde_json::to_string(&range.end_cutters).map_err(|e| e.to_string())?
            ])
            .map_err(|e| e.to_string())?;
        for (letters, events) in events.range_mut(range.start_letters.clone()..=range.end_letters.clone()) {
            let start = if letters == &range.start_letters { range.start_number } else { 0.0 };
            let end = if letters == &range.end_letters { range.end_number } else { f64::INFINITY };
            events.push((start, true, id));
            if end.is_finite() {
                events.push((end, false, id));
            }
        }
    }
    let mut interval = db.prepare("INSERT INTO taxonomy_match_interval VALUES(?1,?2,?3,?4)").map_err(|e| e.to_string())?;
    for (letters, mut events) in events {
        events.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut active = BTreeSet::<i64>::new();
        let mut cursor = 0;
        while cursor < events.len() {
            let number = events[cursor].0;
            let end = cursor + events[cursor..].iter().take_while(|event| event.0 == number).count();
            for &(_, start, id) in &events[cursor..end] {
                if start {
                    active.insert(id);
                }
            }
            interval.execute(params![letters, number, 0, serde_json::to_string(&active).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
            let mut ended = false;
            for &(_, start, id) in &events[cursor..end] {
                if !start {
                    active.remove(&id);
                    ended = true;
                }
            }
            if ended {
                // Ranges include both endpoints. A second row describes the
                // values strictly after an endpoint, including empty gaps.
                interval.execute(params![letters, number, 1, serde_json::to_string(&active).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
            }
            cursor = end;
        }
    }
    Ok(())
}
