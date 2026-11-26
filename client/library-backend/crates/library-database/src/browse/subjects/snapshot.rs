//! Count assignments once, compact transient nodes, then publish packed topology.

use std::collections::{BTreeSet, HashMap};
use std::mem::take;

use library_model::{BrowseHierarchy, BrowseHierarchyNode, BrowseLocation, BrowseRow};
use rusqlite::{Connection, params};

use super::Books;
use crate::browse::{BrowseOptions, SubjectsSql, books::MatchingBooks, ordering};
use crate::{Database, DatabaseError};

pub(in crate::browse) struct SubjectSnapshot {
    pub hierarchy: BrowseHierarchy,
    pub matching: MatchingBooks,
    by_route: HashMap<i64, usize>,
    direct_books: Vec<i64>,
    direct_offsets: Vec<usize>,
    preferred: Vec<bool>,
}

impl SubjectSnapshot {
    pub fn index(&self, route: i64) -> Option<usize> {
        self.by_route.get(&route).copied()
    }

    pub fn direct(&self, node: usize) -> &[i64] {
        &self.direct_books[self.direct_offsets[node]..self.direct_offsets[node + 1]]
    }

    pub fn descendants(&self, node: usize) -> Vec<usize> {
        let mut order = vec![node];
        let mut cursor = 0;
        while cursor < order.len() {
            order.extend(self.hierarchy.children(order[cursor]));
            cursor += 1;
        }
        order
    }

    pub fn members(&self, node: usize) -> BTreeSet<i64> {
        self.descendants(node).into_iter().flat_map(|node| self.direct(node).iter().copied()).collect()
    }

    pub fn selected(&self, node: usize, searched: bool) -> BTreeSet<i64> {
        if searched {
            self.members(node).intersection(&self.matching.text_matches).copied().filter(|book| self.matching.books.contains(book)).collect()
        } else {
            self.direct(node).iter().copied().filter(|book| self.matching.books.contains(book)).collect()
        }
    }

    pub fn children(&self, node: usize, options: &BrowseOptions, catalog: bool) -> Vec<BrowseRow> {
        if options.search.is_empty() {
            return self.hierarchy.children(node).map(|child| &self.hierarchy.nodes[child].row).filter(|row| row.book_count > 0).cloned().collect();
        }
        let indices = self.descendants(node).into_iter().skip(1);
        let mut preferred = HashMap::new();
        let mut rows = Vec::new();
        for index in indices {
            let entry = &self.hierarchy.nodes[index];
            if entry.row.book_count == 0 || !book_model::normalize_search_text(&entry.row.name).contains(&options.search) {
                continue;
            }
            let mut row = entry.row.clone();
            let mut labels = Vec::new();
            let mut current = entry.parent;
            while let Some(parent) = current.filter(|&parent| parent != node) {
                labels.push(self.hierarchy.nodes[parent].row.name.as_str());
                current = self.hierarchy.nodes[parent].parent;
            }
            labels.reverse();
            row.path = (!labels.is_empty()).then(|| labels.join(" \u{203a} "));
            preferred.insert(row.id, self.preferred[index]);
            rows.push(row);
        }
        // Default ordering was route-ID order before presentation sorting.
        rows.sort_by_key(|row| row.id);
        ordering::sort_rows(&mut rows, options.chip_sort);
        if catalog {
            ordering::prefer_rows(&mut rows, &|row| preferred[&row.id]);
        }
        rows
    }
}

#[derive(Default)]
struct BuildNode {
    route: i64,
    parent: Option<usize>,
    children: Vec<usize>,
    total: usize,
    matched: usize,
    downloaded: usize,
    preferred: bool,
    has_direct: bool,
    stamp: usize,
    redirect: Option<usize>,
}

fn resolve(route: i64, indices: &mut HashMap<i64, usize>, nodes: &mut Vec<BuildNode>) -> usize {
    *indices.entry(route).or_insert_with(|| {
        let index = nodes.len();
        nodes.push(BuildNode { route, ..Default::default() });
        index
    })
}

fn redirected(mut node: usize, nodes: &mut [BuildNode]) -> usize {
    let original = node;
    while let Some(next) = nodes[node].redirect {
        node = next;
    }
    if original != node {
        nodes[original].redirect = Some(node);
    }
    node
}

impl Database {
    pub(in crate::browse) fn subject_snapshot(&self, source: Books, catalog: Option<&Connection>, options: Option<&BrowseOptions>) -> Result<SubjectSnapshot, DatabaseError> {
        let mut assignments = Vec::<(i64, i64)>::new();
        match source {
            Books::Library => self.connection.subject_assignments(|row| {
                assignments.push((row.get(0)?, row.get(1)?));
                Ok(())
            })?,
            Books::Catalog => {
                let catalog = catalog.ok_or_else(|| DatabaseError::message("catalog snapshot requires its database"))?;
                crate::attach_taxonomy(catalog)?;
                catalog.catalog_subject_assignments(|row| {
                    assignments.push((row.get(0)?, row.get(1)?));
                    Ok(())
                })?;
            }
        }
        let candidates = assignments.iter().map(|&(book, _)| book).collect::<BTreeSet<_>>();
        let matching = match (options, catalog) {
            (Some(options), Some(catalog)) => crate::browse::catalog::matching_in(catalog, &candidates, &self.library_book_hashes()?, options)?,
            (Some(options), None) => self.matching_books(&candidates, options)?,
            (None, _) => MatchingBooks { books: candidates, downloaded: BTreeSet::new(), text_matches: BTreeSet::new(), preferred: BTreeSet::new() },
        };
        let mut nodes = vec![BuildNode::default()];
        let mut indices = HashMap::from([(0, 0)]);
        let mut parent_query = self.connection.prepare_cached("SELECT parent_route_id FROM curated.unified_concept_route WHERE route_id=?1")?;
        let mut assigned_nodes = Vec::with_capacity(assignments.len());
        for (book, route) in assignments {
            let assigned = resolve(route, &mut indices, &mut nodes);
            let mut node = assigned;
            while node != 0 && nodes[node].parent.is_none() {
                let parent: i64 = parent_query.query_row(params![nodes[node].route], |row| row.get(0))?;
                let parent = resolve(parent, &mut indices, &mut nodes);
                nodes[node].parent = Some(parent);
                node = parent;
            }
            assigned_nodes.push((book, assigned));
            nodes[assigned].has_direct = true;
        }
        for node in 1..nodes.len() {
            let parent = nodes[node].parent.ok_or_else(|| DatabaseError::message("subject has no parent"))?;
            nodes[parent].children.push(node);
        }
        let mut order = vec![0];
        let mut cursor = 0;
        while cursor < order.len() {
            order.extend_from_slice(&nodes[order[cursor]].children);
            cursor += 1;
        }
        if order.len() != nodes.len() {
            return Err(DatabaseError::message("subject taxonomy contains a parent cycle"));
        }
        // Unfiltered unique counts keep compaction independent of filters.
        let mut previous = None;
        let mut stamp = 0;
        for &(book, assigned) in &assigned_nodes {
            if previous != Some(book) {
                stamp += 1;
                previous = Some(book);
            }
            let mut node = assigned;
            loop {
                let entry = &mut nodes[node];
                if entry.stamp == stamp {
                    break;
                }
                entry.stamp = stamp;
                entry.total += 1;
                let Some(parent) = entry.parent else { break };
                node = parent;
            }
        }
        for &node in order.iter().rev().filter(|&&node| node != 0) {
            let children = take(&mut nodes[node].children);
            nodes[node].children = children.into_iter().filter(|&child| nodes[child].redirect.is_none()).collect();
            if nodes[node].children.is_empty() && nodes[node].total <= 2 {
                let parent = nodes[node].parent.expect("non-root subject has a parent");
                nodes[node].redirect = Some(parent);
                nodes[parent].has_direct |= nodes[node].total != 0;
            } else if !nodes[node].has_direct && nodes[node].children.len() == 1 {
                let child = nodes[node].children[0];
                let grandchildren = take(&mut nodes[child].children);
                for &grandchild in &grandchildren {
                    nodes[grandchild].parent = Some(node);
                }
                nodes[node].has_direct = nodes[child].has_direct;
                nodes[node].children = grandchildren;
                nodes[child].redirect = Some(node);
            }
        }
        // Count each book once per surviving container. Ancestors add their
        // children's counts, including books shared by different children.
        let mut memberships = assigned_nodes.into_iter().map(|(book, node)| (redirected(node, &mut nodes), book)).collect::<Vec<_>>();
        memberships.sort_unstable();
        memberships.dedup();
        for &(node, book) in &memberships {
            nodes[node].matched += usize::from(matching.books.contains(&book));
            nodes[node].downloaded += usize::from(matching.downloaded.contains(&book));
            nodes[node].preferred |= matching.preferred.contains(&book);
        }
        // Compaction only reparents nodes to earlier ancestors, so the
        // original reverse order still visits every child before its parent.
        for &node in order.iter().rev() {
            if nodes[node].redirect.is_some() {
                continue;
            }
            let Some(parent) = nodes[node].parent else { continue };
            let (matched, downloaded, preferred) = (nodes[node].matched, nodes[node].downloaded, nodes[node].preferred);
            nodes[parent].matched += matched;
            nodes[parent].downloaded += downloaded;
            nodes[parent].preferred |= preferred;
        }
        let mut labels = HashMap::new();
        let routes = nodes.iter().filter(|node| node.redirect.is_none() && node.route != 0).map(|node| node.route).collect::<Vec<_>>();
        self.connection.taxonomy_route_labels(&serde_json::to_string(&routes).map_err(DatabaseError::operation)?, |row| {
            labels.insert(row.get::<_, i64>(0)?, row.get::<_, String>(1)?);
            Ok(())
        })?;
        let mut packed = vec![0];
        let mut packed_indices = vec![usize::MAX; nodes.len()];
        packed_indices[0] = 0;
        let mut output = Vec::new();
        let mut preferred = Vec::new();
        let mut cursor = 0;
        while cursor < packed.len() {
            let node = packed[cursor];
            let first_child = packed.len();
            let children = take(&mut nodes[node].children);
            let mut children = children.into_iter().filter(|&child| nodes[child].redirect.is_none()).collect::<Vec<_>>();
            children.sort_by_key(|&child| nodes[child].route);
            let mut rows = children
                .iter()
                .map(|&child| BrowseRow {
                    id: source.location(nodes[child].route),
                    name: labels[&nodes[child].route].clone(),
                    path: None,
                    book_count: nodes[child].matched,
                    downloaded_book_count: nodes[child].downloaded,
                    requested_book_count: None,
                    size_bytes: None,
                })
                .collect::<Vec<_>>();
            if let Some(options) = options {
                ordering::sort_rows(&mut rows, options.chip_sort);
                if source == Books::Catalog {
                    ordering::prefer_rows(&mut rows, &|row| nodes[indices[&route_of(row.id)]].preferred);
                }
                children = rows.into_iter().map(|row| indices[&route_of(row.id)]).collect();
            }
            for child in children {
                packed_indices[child] = packed.len();
                packed.push(child);
            }
            let entry = &nodes[node];
            output.push(BrowseHierarchyNode {
                row: BrowseRow {
                    id: source.location(entry.route),
                    name: if node == 0 { "Subject".into() } else { labels.remove(&entry.route).ok_or_else(|| DatabaseError::message("subject label is missing"))? },
                    path: None,
                    book_count: entry.matched,
                    downloaded_book_count: entry.downloaded,
                    requested_book_count: None,
                    size_bytes: None,
                },
                parent: entry.parent.map(|parent| packed_indices[parent]),
                first_child,
            });
            preferred.push(entry.preferred);
            cursor += 1;
        }
        let mut memberships = memberships.into_iter().map(|(node, book)| (packed_indices[node], book)).collect::<Vec<_>>();
        memberships.sort_unstable();
        let mut direct_books = Vec::with_capacity(memberships.len());
        let mut direct_offsets = Vec::with_capacity(output.len() + 1);
        let mut members = memberships.into_iter().peekable();
        for node in 0..output.len() {
            direct_offsets.push(direct_books.len());
            while members.peek().is_some_and(|&(index, _)| index == node) {
                direct_books.push(members.next().expect("peeked membership").1);
            }
        }
        direct_offsets.push(direct_books.len());
        let hierarchy = BrowseHierarchy::new(output).map_err(DatabaseError::message)?;
        let by_route = hierarchy.nodes.iter().enumerate().map(|(index, node)| (route_of(node.row.id), index)).collect();
        Ok(SubjectSnapshot { hierarchy, matching, by_route, direct_books, direct_offsets, preferred })
    }
}

fn route_of(location: BrowseLocation) -> i64 {
    match location {
        BrowseLocation::Subject(route) | BrowseLocation::Catalog(route) => route,
        _ => unreachable!("subject snapshot only contains subject routes"),
    }
}
