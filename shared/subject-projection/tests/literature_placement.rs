#![cfg(all(feature = "authoritative-taxonomy", not(target_arch = "wasm32")))]

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;
use serde_json::Value;
use subject_projection::{project_subject_codes, project_subject_concepts, read_unified_taxonomy_connection, reviewed_source_heading_path, unified_concept_id, unified_subject_paths, unified_taxonomy_routes, BookSubject, UnifiedConceptDefinition, UnifiedTaxonomy};

const LITERATURE: i64 = 3846;
const PLAN: &str = include_str!("../data/literature-cleanup-20261014.json");

fn bundled_definitions() -> Vec<UnifiedConceptDefinition> {
    let db = Connection::open_with_flags(subject_projection::bundled::path().unwrap(), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    read_unified_taxonomy_connection(&db, "test taxonomy").unwrap()
}

fn roots(id: i64, concepts: &BTreeMap<i64, UnifiedConceptDefinition>) -> BTreeSet<i64> {
    let mut pending = vec![id];
    let mut visited = BTreeSet::new();
    let mut roots = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let parents = concepts[&id].parent_ids();
        if parents.is_empty() {
            roots.insert(id);
        } else {
            pending.extend(parents.iter().copied());
        }
    }
    roots
}

fn numbered_code(code: &str) -> Option<(&str, f64)> {
    let index = code.find(|ch: char| ch.is_ascii_digit())?;
    Some((&code[..index], code[index..].parse().ok()?))
}

#[test]
fn every_reviewed_literary_scope_resolves_exclusively_under_literature() {
    let plan: Value = serde_json::from_str(PLAN).unwrap();
    let definitions = bundled_definitions();
    let taxonomy = UnifiedTaxonomy::from_concepts(definitions.clone()).unwrap();
    let concepts = definitions.into_iter().map(|c| (c.concept_id(), c)).collect();

    for scope in plan["reviewed_scopes"].as_array().unwrap() {
        let start = scope["start"].as_str().unwrap();
        let end = scope["end"].as_str().unwrap();
        let mut probes = vec![start.to_owned(), end.to_owned()];
        if let (Some((a, low)), Some((b, high))) = (numbered_code(start), numbered_code(end)) {
            if a == b {
                probes.push(format!("{a}{}", (low + high) / 2.0));
            }
        }
        for code in probes {
            let matches = taxonomy.matching_concept_ids("lcc", &code);
            assert!(!matches.is_empty(), "reviewed literary code {code} is unresolved");
            for id in matches {
                assert_eq!(roots(id, &concepts), BTreeSet::from([LITERATURE]), "{code}: {}", concepts[&id].preferred_label());
            }
        }
    }
}

#[test]
fn literary_subjects_and_literature_selectors_have_no_nonliterary_route() {
    let plan: Value = serde_json::from_str(PLAN).unwrap();
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let moved = plan["moved_literary_concepts"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap());
    let added = plan["new_concepts"].as_array().unwrap().iter().map(|v| v["id"].as_i64().unwrap());
    let flattened: Value = serde_json::from_str(include_str!("../data/literature-flatten-speculative-20261015.json")).unwrap();
    let forms: Value = serde_json::from_str(include_str!("../data/literature-flatten-literary-forms-20261015.json")).unwrap();
    let mut removed_groups = BTreeSet::from([flattened["removed_concept_id"].as_i64().unwrap()]);
    removed_groups.extend(forms["stages"].as_array().unwrap().iter().map(|stage| stage["removed_concept_id"].as_i64().unwrap()));
    let relationships: Value = serde_json::from_str(include_str!("../data/literature-relationships-leaf-20261015.json")).unwrap();
    removed_groups.extend(relationships["merged_concepts"].as_array().unwrap().iter().map(|item| item["concept_id"].as_i64().unwrap()));
    let humor: Value = serde_json::from_str(include_str!("../data/humor-flatten-form-topic-20261015.json")).unwrap();
    removed_groups.extend(humor["stages"].as_array().unwrap().iter().map(|stage| stage["removed_concept_id"].as_i64().unwrap()));
    let english_forms: Value = serde_json::from_str(include_str!("../data/literature-english-forms-great-britain-20261015.json")).unwrap();
    removed_groups.insert(english_forms["removed_concept_id"].as_i64().unwrap());
    for id in &removed_groups {
        assert!(!concepts.contains_key(id), "the uncoded grouping was flattened");
    }
    for id in moved.chain(added).filter(|id| !removed_groups.contains(id)) {
        assert_eq!(roots(id, &concepts), BTreeSet::from([LITERATURE]), "{}", concepts[&id].preferred_label());
    }
    for concept in concepts.values() {
        let literary_ddc = concept.source_selectors("ddc").iter().any(|code| code.starts_with('8'));
        let literary_bisac = concept.source_selectors("bisac").iter().any(|code| ["FIC", "LIT", "LCO", "POE", "DRA"].iter().any(|prefix| code.starts_with(prefix)));
        if literary_ddc || literary_bisac {
            assert_eq!(roots(concept.concept_id(), &concepts), BTreeSet::from([LITERATURE]), "{}", concept.preferred_label());
        }
    }
}

#[test]
fn speculative_children_keep_their_codes_and_saved_paths_directly_under_genres() {
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let taxonomy = UnifiedTaxonomy::from_concepts(concepts.values().cloned().collect()).unwrap();
    for (id, label, bisac) in [
        (3986, "Fantasy", "FIC009000"),
        (4026, "LitRPG", "FIC129000"),
        (4108, "Science Fiction", "FIC028000"),
        (4128, "Superheroes", "FIC063000"),
        (4144, "Visionary & Metaphysical", "FIC039000"),
    ] {
        assert_eq!(concepts[&id].parent_ids(), &[5583]);
        assert_eq!(taxonomy.matching_concept_ids("bisac", bisac), vec![id]);
        assert_eq!(unified_concept_id(&format!("Literature / Genres / {label}")), Some(id));
        let old = format!("Literature / Genres / Speculative / {label}");
        assert_eq!(unified_concept_id(&old), Some(id));
        let included = BookSubject::new(None, label, "manual:subject", Some("unified".into()), Some(old.clone())).unwrap();
        let excluded = BookSubject::new(None, label, "manual:subject:exclude", Some("unified".into()), Some(old)).unwrap();
        assert_eq!(project_subject_concepts(&project_subject_codes(std::slice::from_ref(&included))).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([id]));
        assert!(project_subject_concepts(&project_subject_codes(&[included, excluded])).is_empty());
    }
    assert!(unified_taxonomy_routes().iter().all(|route| !route.path().starts_with("Literature / Genres / Speculative")));
    // The separately reviewed Mystery & Thrillers grouping stays intact.
    assert_eq!(concepts[&55023].parent_ids(), &[5583]);
    for child in [4025, 4034, 4056, 4129] {
        assert_eq!(concepts[&child].parent_ids(), &[55023]);
    }
}

#[test]
fn language_study_and_the_retained_youth_and_comics_branches_keep_their_scopes() {
    for code in ["PJ1200", "PJ4900", "PK6200", "PK8500", "PC790", "PC930", "PL4641", "PL4072"] {
        let paths = unified_subject_paths("lcc", code);
        assert!(!paths.is_empty(), "{code}");
        assert!(paths.iter().all(|path| path == "Language" || path.starts_with("Language / ")), "{code}: {paths:?}");
    }
    for (system, code, root) in [("lcc", "PZ7", "Youth / "), ("bisac", "JUV000000", "Youth / "), ("bisac", "YAF000000", "Youth / "), ("bisac", "CGN000000", "Arts & Media / Comics")] {
        let paths = unified_subject_paths(system, code);
        assert!(paths.iter().any(|path| path.starts_with(root)), "{system}:{code}: {paths:?}");
    }
}

#[test]
fn specific_literary_navigation_shadows_general_literature_but_keeps_independent_language_study() {
    let subject = |system: &str, code: &str| BookSubject::new(None, code, "catalog:test", Some(system.into()), Some(code.into())).unwrap();
    let subjects = [subject("lcc", "PJ"), subject("lcc", "PJ1482 .B8"), subject("lcc", "PJ1200"), subject("bisac", "FIC000000")];
    let codes = project_subject_codes(&subjects);
    assert!(!codes.iter().any(|code| code.code() == "PJ"), "the broad Literature assignment is shadowed by specific literary evidence");
    let concepts = project_subject_concepts(&codes).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>();
    let paths = unified_taxonomy_routes().into_iter().filter(|route| concepts.contains(&route.concept_id())).map(|route| route.path().to_owned()).collect::<Vec<_>>();
    assert!(!paths.iter().any(|path| path == "Literature"));
    assert!(paths.iter().any(|path| path.starts_with("Literature / ")), "{paths:?}");
    assert!(paths.iter().any(|path| path == "Language" || path.starts_with("Language / ")), "{paths:?}");

    let heading = BookSubject::new(None, "Hebrew literature -- Translations into English", "catalog:gutenberg.org", Some("lcsh".into()), None).unwrap();
    assert_eq!(reviewed_source_heading_path(&heading), Some("Literature / By Language / Hebrew"));
}

#[test]
fn catalog_literature_codes_cover_missing_interiors_and_keep_period_boundaries() {
    let taxonomy = UnifiedTaxonomy::from_concepts(bundled_definitions()).unwrap();
    for (code, expected) in [
        ("PR6019", 4179),
        ("PR6019.O9 D8 2001", 4179), // Dubliners
        ("PR6045.O53 R5 1986", 4179), // Right Ho, Jeeves
        ("PR6052", 4179),
        ("PR6076.Z9", 4179),
        ("PR6077", 5585), // outside the documented 20th-century intervals
        ("PR6103.A67 H68 2012", 4180),
        ("PR6126.Z9", 4180),
        ("PR6127", 5585),
        ("PR9680", 5585),
        ("PQ6329", 4226),
        ("PQ9251", 4215),
        ("PQ1100", 4295),
        ("PQ1278", 4295),
        ("PS8476", 3867),
        ("PS8649", 3867),
        ("PL3512.45", 4225),
    ] {
        assert_eq!(taxonomy.matching_concept_ids("lcc", code), vec![expected], "{code}");
    }
    // The broad English fallback must not replace an existing narrower period.
    assert_eq!(taxonomy.matching_concept_ids("lcc", "PR4550 .A1 2000"), vec![3915]);
    // Do not turn glued publication years or out-of-schedule numbers into
    // supported classes by accepting arbitrary alphabetic prefix matches.
    for code in ["PR9681", "PR35602014", "PQ43022 F87", "PS32012015"] {
        assert!(taxonomy.matching_concept_ids("lcc", code).is_empty(), "{code}");
    }
}

#[test]
fn reviewed_literary_headings_follow_their_pinned_subject_identities() {
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let rules: Value = serde_json::from_str(include_str!("../data/reviewed-source-headings.json")).unwrap();
    for rule in rules.as_array().unwrap() {
        let id = rule["concept_id"].as_i64().unwrap();
        if roots(id, &concepts).contains(&LITERATURE) {
            let destination = rule["destination"].as_str().unwrap();
            assert_eq!(unified_concept_id(destination), Some(id), "stale literary heading route: {}", rule["raw"]);
        }
    }
    for (heading, path) in [
        ("Hebrew literature -- Translations into English", "Literature / By Language / Hebrew"),
        ("French fiction -- Translations into English", "Literature / By Language / French"),
        ("Old Norse literature -- Translations into English", "Literature / By Language / Nordic / Old Icelandic & Norwegian"),
        ("American fiction -- 19th century", "Literature / By Language / English / U.S. / By Period / 19th Century"),
    ] {
        let subject = BookSubject::new(None, heading, "dc:subject", None, None).unwrap();
        assert_eq!(reviewed_source_heading_path(&subject), Some(path), "{heading}");
    }
}

#[test]
fn historical_literary_paths_preserve_manual_assignments_and_exclusions() {
    let aliases: Value = serde_json::from_str(include_str!("../data/reviewed-literature-path-aliases.json")).unwrap();
    for alias in aliases["aliases"].as_array().unwrap() {
        let path = alias["path"].as_str().unwrap();
        let id = alias["concept_id"].as_i64().unwrap();
        assert_eq!(unified_concept_id(path), Some(id), "{path}");
        let paths = unified_subject_paths("unified", path);
        assert!(!paths.is_empty(), "{path}");
        assert!(paths.iter().all(|current| unified_concept_id(current) == Some(id)), "{path}");
    }
    let old_swedish = "Literature / By Region / Nordic / Sweden / Swedish Literary Studies / Swedish Literary Criticism";
    let subject = BookSubject::new(None, "Swedish Literary Criticism", "manual:subject", Some("unified".into()), Some(old_swedish.into())).unwrap();
    let codes = project_subject_codes(std::slice::from_ref(&subject));
    assert_eq!(project_subject_concepts(&codes).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([8951]));
    assert_eq!(subject.code(), Some(old_swedish), "the original curation evidence remains intact");

    let subjects = [
        BookSubject::new(None, "PS8476", "metadata:work", Some("lcc".into()), Some("PS8476".into())).unwrap(),
        BookSubject::new(None, "Canadian", "manual:subject:exclude", Some("unified".into()), Some("Literature / By Region / American & Caribbean / Canadian".into())).unwrap(),
    ];
    assert!(project_subject_concepts(&project_subject_codes(&subjects)).is_empty(), "the saved Canadian exclusion still applies to the same identity");
    assert!(unified_subject_paths("unified", "Literature / Invented old subject").is_empty());
}

#[test]
fn adult_literary_forms_and_traditions_keep_codes_and_saved_assignments_under_genres() {
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let taxonomy = UnifiedTaxonomy::from_concepts(concepts.values().cloned().collect()).unwrap();
    let plan: Value = serde_json::from_str(include_str!("../data/literature-flatten-literary-forms-20261015.json")).unwrap();
    let removed = BTreeSet::from([55027, 5630]);
    for stage in plan["stages"].as_array().unwrap() {
        let group = stage["removed_concept_id"].as_i64().unwrap();
        assert!(!concepts.contains_key(&group));
        for child in stage["child_concept_ids"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).filter(|id| !removed.contains(id)) {
            assert_eq!(concepts[&child].parent_ids(), &[5583]);
        }
    }
    for (code, id) in [("DRA000000", 3876), ("LCO000000", 4280), ("POE000000", 4416), ("FIC029000", 4124)] {
        assert_eq!(taxonomy.matching_concept_ids("bisac", code), vec![id]);
    }
    for alias in plan["historical_path_aliases"].as_array().unwrap() {
        let path = alias["path"].as_str().unwrap();
        let id = alias["concept_id"].as_i64().unwrap();
        let included = BookSubject::new(None, path, "manual:subject", Some("unified".into()), Some(path.into())).unwrap();
        let excluded = BookSubject::new(None, path, "manual:subject:exclude", Some("unified".into()), Some(path.into())).unwrap();
        assert_eq!(project_subject_concepts(&project_subject_codes(std::slice::from_ref(&included))).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([id]), "{path}");
        assert!(project_subject_concepts(&project_subject_codes(&[included, excluded])).is_empty(), "{path}");
    }
    assert!(unified_taxonomy_routes().iter().all(|route| !route.path().starts_with("Literature / Genres / Literary Forms") && !route.path().starts_with("Literature / Genres / Forms & Literary Traditions")));
    // The youth form grouping has its own scope and remains unchanged.
    assert_eq!(concepts[&55252].parent_ids(), &[5635]);
    for child in [1749, 1777, 1823, 1835, 1843, 1869, 1907] {
        assert_eq!(concepts[&child].parent_ids(), &[55252]);
    }
}

#[test]
fn relationships_is_a_leaf_and_erotica_keeps_its_independent_subtree() {
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let taxonomy = UnifiedTaxonomy::from_concepts(concepts.values().cloned().collect()).unwrap();
    let plan: Value = serde_json::from_str(include_str!("../data/literature-relationships-leaf-20261015.json")).unwrap();
    assert_eq!(concepts[&55024].parent_ids(), &[5583]);
    assert_eq!(concepts[&3972].parent_ids(), &[5583]);
    assert!(!concepts.values().any(|concept| concept.parent_ids().contains(&55024)));
    assert_eq!(unified_concept_id("Literature / Genres / Relationships"), Some(55024));
    assert_eq!(unified_concept_id("Literature / Genres / Erotica"), Some(3972));
    for merged in plan["merged_concepts"].as_array().unwrap() {
        assert!(!concepts.contains_key(&merged["concept_id"].as_i64().unwrap()));
    }
    for code in plan["bisac_remapping"].as_array().unwrap() {
        let code = code["code"].as_str().unwrap();
        assert_eq!(taxonomy.matching_concept_ids("bisac", code), vec![55024], "{code}");
    }
    for alias in plan["historical_path_aliases"].as_array().unwrap() {
        let path = alias["path"].as_str().unwrap();
        let id = alias["concept_id"].as_i64().unwrap();
        let included = BookSubject::new(None, path, "manual:subject", Some("unified".into()), Some(path.into())).unwrap();
        let excluded = BookSubject::new(None, path, "manual:subject:exclude", Some("unified".into()), Some(path.into())).unwrap();
        assert_eq!(unified_concept_id(path), Some(id));
        assert_eq!(project_subject_concepts(&project_subject_codes(std::slice::from_ref(&included))).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([id]), "{path}");
        assert!(project_subject_concepts(&project_subject_codes(&[included, excluded])).is_empty(), "{path}");
    }
    let erotica: BTreeSet<_> = plan["retained_erotica_concept_ids"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap()).collect();
    for id in &erotica {
        for code in concepts[id].source_selectors("bisac") {
            assert_eq!(taxonomy.matching_concept_ids("bisac", code), vec![*id]);
        }
    }
    for child in [3973, 3974, 3975, 3976, 3981] {
        assert!(erotica.contains(&child));
        assert_eq!(concepts[&child].parent_ids(), &[3972]);
    }
    let subjects = [
        BookSubject::new(None, "Romance", "manual:subject", Some("unified".into()), Some("Literature / Genres / Relationships / Romance".into())).unwrap(),
        BookSubject::new(None, "Family Life Fiction", "manual:subject", Some("unified".into()), Some("Literature / Genres / Relationships / Family Life Fiction".into())).unwrap(),
        BookSubject::new(None, "Erotica", "catalog:test", Some("bisac".into()), Some("FIC005000".into())).unwrap(),
    ];
    assert_eq!(project_subject_concepts(&project_subject_codes(&subjects)).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([55024, 3972]));
}

#[test]
fn humor_form_and_topic_children_keep_codes_and_saved_paths_in_both_routes() {
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let taxonomy = UnifiedTaxonomy::from_concepts(concepts.values().cloned().collect()).unwrap();
    let plan: Value = serde_json::from_str(include_str!("../data/humor-flatten-form-topic-20261015.json")).unwrap();
    assert_eq!(concepts[&691].parent_ids(), &[510, 55128]);
    let children: BTreeSet<_> = plan["stages"].as_array().unwrap().iter().flat_map(|stage| stage["child_concept_ids"].as_array().unwrap()).map(|v| v.as_i64().unwrap()).collect();
    assert_eq!(children.len(), 25);
    assert_eq!(concepts.values().filter(|c| c.parent_ids().contains(&691)).map(|c| c.concept_id()).collect::<BTreeSet<_>>(), children);
    for stage in plan["stages"].as_array().unwrap() {
        assert!(!concepts.contains_key(&stage["removed_concept_id"].as_i64().unwrap()));
    }
    assert_eq!(taxonomy.matching_concept_ids("bisac", "HUM000000"), vec![691]);
    for id in &children {
        assert_eq!(concepts[id].parent_ids(), &[691]);
        for code in concepts[id].source_selectors("bisac") {
            assert_eq!(taxonomy.matching_concept_ids("bisac", code), vec![*id], "{code}");
        }
    }
    for id in [53185, 53186] {
        assert_eq!(concepts[&id].parent_ids(), &[713]);
    }
    for alias in plan["historical_path_aliases"].as_array().unwrap() {
        let path = alias["path"].as_str().unwrap();
        let id = alias["concept_id"].as_i64().unwrap();
        assert_eq!(unified_concept_id(path), Some(id));
        let included = BookSubject::new(None, path, "manual:subject", Some("unified".into()), Some(path.into())).unwrap();
        let excluded = BookSubject::new(None, path, "manual:subject:exclude", Some("unified".into()), Some(path.into())).unwrap();
        assert_eq!(project_subject_concepts(&project_subject_codes(std::slice::from_ref(&included))).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([id]), "{path}");
        assert!(project_subject_concepts(&project_subject_codes(&[included, excluded])).is_empty(), "{path}");
    }
    for prefix in ["Arts & Media / Humor", "Literature / Genres / Humorous Writing / Humor"] {
        assert_eq!(unified_concept_id(&format!("{prefix} / Anecdotes & Quotations")), Some(693));
        let subjects = [
            BookSubject::new(None, "Form", "manual:subject", Some("unified".into()), Some(format!("{prefix} / Form"))).unwrap(),
            BookSubject::new(None, "Anecdotes & Quotations", "manual:subject", Some("unified".into()), Some(format!("{prefix} / Form / Anecdotes & Quotations"))).unwrap(),
        ];
        assert_eq!(project_subject_concepts(&project_subject_codes(&subjects)).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([693]));
    }
}

#[test]
fn english_forms_keep_saved_paths_under_great_britain_and_shadow_broad_english() {
    let concepts: BTreeMap<_, _> = bundled_definitions().into_iter().map(|c| (c.concept_id(), c)).collect();
    let plan: Value = serde_json::from_str(include_str!("../data/literature-english-forms-great-britain-20261015.json")).unwrap();
    assert!(!concepts.contains_key(&55178));
    assert_eq!(concepts[&60178].parent_ids(), &[5585]);
    for edge in plan["child_edges"].as_array().unwrap() {
        assert_eq!(concepts[&edge[0].as_i64().unwrap()].parent_ids(), &[60178]);
    }
    for id in [7702, 55177, 6595, 8297] {
        assert_eq!(concepts[&id].parent_ids(), &[60178]);
    }
    for alias in plan["historical_path_aliases"].as_array().unwrap() {
        let path = alias["path"].as_str().unwrap();
        let id = alias["concept_id"].as_i64().unwrap();
        assert_eq!(unified_concept_id(path), Some(id));
        let included = BookSubject::new(None, path, "manual:subject", Some("unified".into()), Some(path.into())).unwrap();
        let excluded = BookSubject::new(None, path, "manual:subject:exclude", Some("unified".into()), Some(path.into())).unwrap();
        assert_eq!(project_subject_concepts(&project_subject_codes(std::slice::from_ref(&included))).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([id]), "{path}");
        assert!(project_subject_concepts(&project_subject_codes(&[included, excluded])).is_empty(), "{path}");
    }
    assert_eq!(unified_concept_id("Literature / By Language / English / Great Britain / Drama"), Some(3906));
    let subjects = [
        BookSubject::new(None, "English", "manual:subject", Some("unified".into()), Some("Literature / By Language / English".into())).unwrap(),
        BookSubject::new(None, "Drama", "manual:subject", Some("unified".into()), Some("Literature / By Language / English Forms / Drama".into())).unwrap(),
    ];
    assert_eq!(project_subject_concepts(&project_subject_codes(&subjects)).into_iter().map(|(_, id)| id).collect::<BTreeSet<_>>(), BTreeSet::from([3906]));
}
