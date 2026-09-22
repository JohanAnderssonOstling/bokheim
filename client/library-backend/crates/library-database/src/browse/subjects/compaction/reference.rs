//! Previous all-card, string-path implementation, retained only as a regression oracle.
use super::*;
use library_model::{BrowseContents, LibraryBrowseQuery};
use std::collections::HashMap;

#[derive(Default)]
struct PathNode {
    parent: String,
    label: String,
    children: BTreeSet<String>,
    members: BTreeSet<usize>,
    direct: BTreeSet<usize>,
}

fn compact(path: &str, nodes: &BTreeMap<String, PathNode>) -> (BTreeSet<usize>, BTreeSet<String>) {
    let Some(node) = nodes.get(path) else { return Default::default() };
    let mut books = node.direct.clone();
    let mut subjects = BTreeSet::new();
    for child in &node.children {
        let branch = &nodes[child];
        if branch.members.is_empty() {
            continue;
        }
        if branch.members.len() <= 3 {
            books.extend(&branch.members);
            continue;
        }
        let (nested_books, nested_subjects) = compact(child, nodes);
        if nested_books.is_empty() && nested_subjects.len() == 1 {
            subjects.extend(nested_subjects);
        } else {
            subjects.insert(child.clone());
        }
    }
    (books, subjects)
}

pub(super) fn browse(db: &Database, query: &LibraryBrowseQuery, downloaded_only: bool) -> Result<BrowseContents, DatabaseError> {
    db.with_subject_cache(|| {
        let location = if query.location.is_empty() { ROOT_SUBJECT_PATH } else { &query.location };
        let route = db.subject_route_id(location)?.unwrap_or(-1);
        let options = crate::browse::BrowseOptions::from_query(query, downloaded_only);
        let read = |downloaded: bool, formats: i32, languages: &str| -> Result<Vec<library_model::BookCardRow>, DatabaseError> {
            let mut statement = db.connection.prepare_cached("SELECT card.*,NULL AS source_directory FROM subject_browse_member member JOIN book_cards card ON card.book_row_id=member.book_row_id WHERE member.route_id=?1 AND (?2=0 OR card.downloaded) AND (?3=0 OR (?3 & card.format_category)!=0) AND (?4='' OR EXISTS(SELECT 1 FROM book_browse_language language WHERE language.book_row_id=card.book_row_id AND instr(?4,',' || language.base_language || ',')>0)) ORDER BY CASE WHEN ?5=1 THEN (SELECT MAX(reading.changed_at) FROM sync_state_version reading WHERE reading.state_kind='reading_position' AND reading.state_key=card.content_hash) END DESC,card.sort_title,card.title,card.content_hash")?;
            Ok(statement.query_map(rusqlite::params![route, downloaded, formats, languages, book_sort_query_value(query.book_sort)], read_book_card_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
        };
        let cards = read(false, 0, "")?;
        let filtered = read(downloaded_only, options.file_types, &options.languages)?;
        let matching_hashes = filtered.into_iter().filter(|card| !query.hide_finished || card.progress < library_model::FINISHED_PROGRESS_THRESHOLD).map(|card| card.content_hash).collect::<std::collections::HashSet<_>>();
        let mut matching = cards.iter().enumerate().filter_map(|(index, card)| matching_hashes.contains(&card.content_hash).then_some(index)).collect::<BTreeSet<_>>();
        let indices = cards.iter().enumerate().map(|(index, card)| (card.content_hash.to_string(), index)).collect::<HashMap<_, _>>();
        let hashes = serde_json::to_string(&cards.iter().map(|card| card.content_hash.as_str()).collect::<Vec<_>>()).map_err(DatabaseError::operation)?;
        let mut nodes = BTreeMap::<String, PathNode>::new();
        let mut statement = db.connection.prepare_cached("SELECT route.path,route.parent_path,route.label,book.content_hash,member.direct FROM json_each(?1) selected JOIN book ON book.content_hash=selected.value JOIN subject_browse_member member ON member.book_row_id=book.row_id JOIN subject_route_paths route ON route.route_id=member.route_id")?;
        let mut rows = statement.query([&hashes])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            if location != ROOT_SUBJECT_PATH && path != location && !path.starts_with(&format!("{location} / ")) { continue; }
            let hash: String = row.get(3)?;
            let index = indices[&hash];
            let node = nodes.entry(path).or_default();
            node.parent = row.get(1)?;
            node.label = row.get(2)?;
            node.members.insert(index);
            if row.get::<_, bool>(4)? { node.direct.insert(index); }
        }
        let edges = nodes.iter().map(|(path, node)| (path.clone(), node.parent.clone())).collect::<Vec<_>>();
        for (path, parent) in edges {
            if path != parent {
                if let Some(node) = nodes.get_mut(&parent) { node.children.insert(path); }
            }
        }
        let (mut shown_books, shown_subjects) = compact(location, &nodes);
        if query.include_direct_child_books {
            if let Some(root) = nodes.get(location) {
                for child in &root.children { shown_books.extend(&nodes[child].direct); }
            }
        }
        let search = book_model::normalize_search_text(&query.search);
        if !search.is_empty() {
            let mut matches = cards.iter().enumerate().filter_map(|(index, card)| {
                let title = format!("{}{}", card.title, card.subtitle.as_ref().map(|s| format!(": {s}")).unwrap_or_default());
                (book_model::normalize_search_text(&title).contains(&search) || book_model::normalize_search_text(&card.author).contains(&search)).then_some(index)
            }).collect::<BTreeSet<_>>();
            for (path, node) in &nodes {
                if path != location && book_model::normalize_search_text(&node.label).contains(&search) { matches.extend(&node.members); }
            }
            matching.retain(|index| matches.contains(index));
        }
        for node in nodes.values_mut() { node.members.retain(|index| matching.contains(index)); }
        shown_books.retain(|index| matching.contains(index));
        let mut children = shown_subjects.into_iter().filter(|path| !nodes[path].members.is_empty()).map(|path| {
            let node = &nodes[&path];
            BrowseRow { id: path, name: node.label.clone(), path: (node.parent != location).then(|| node.parent.clone()), book_count: node.members.len(), downloaded_book_count: node.members.iter().filter(|index| cards[**index].downloaded).count() }
        }).collect::<Vec<_>>();
        children.sort_by(|left, right| {
            let counts = if query.chip_sort == BrowseChipSort::BookCount { right.book_count.cmp(&left.book_count) } else { std::cmp::Ordering::Equal };
            counts.then_with(|| book_model::normalize_search_text(&left.name).cmp(&book_model::normalize_search_text(&right.name))).then_with(|| left.name.cmp(&right.name)).then_with(|| left.id.cmp(&right.id))
        });
        let books = cards.into_iter().enumerate().filter_map(|(index, card)| shown_books.contains(&index).then_some(card)).collect();
        Ok(BrowseContents { children, books })
    })
}
