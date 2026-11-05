//! Static interval tree over class letters and numbers. Cutter checks remain
//! with the matcher: this index only removes numerically impossible candidates.
use crate::lcc::compare_lcc_key;
use super::LccSelector;
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IntervalIndex {
    // Sorted by lower endpoint. Each slice's midpoint is its implicit root.
    nodes: Vec<Node>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Node {
    range: usize,
    // Original range with the greatest upper endpoint in this subtree.
    max_end: usize,
}

impl IntervalIndex {
    pub fn build(ranges: &[LccSelector]) -> Self {
        let mut nodes: Vec<_> = (0..ranges.len()).map(|range| Node { range, max_end: range }).collect();
        nodes.sort_by(|a, b| {
            let (a_range, b_range) = (&ranges[a.range], &ranges[b.range]);
            compare_lcc_key(&a_range.start_letters, a_range.start_number, &b_range.start_letters, b_range.start_number).then_with(|| a.range.cmp(&b.range))
        });
        fn augment(nodes: &mut [Node], ranges: &[LccSelector]) -> Option<usize> {
            if nodes.is_empty() {
                return None;
            }
            let middle = nodes.len() / 2;
            let (left, rest) = nodes.split_at_mut(middle);
            let (root, right) = rest.split_first_mut()?;
            let mut max_end = root.range;
            for candidate in [augment(left, ranges), augment(right, ranges)].into_iter().flatten() {
                let (a, b) = (&ranges[candidate], &ranges[max_end]);
                if compare_lcc_key(&a.end_letters, a.end_number, &b.end_letters, b.end_number).is_gt() {
                    max_end = candidate;
                }
            }
            root.max_end = max_end;
            Some(max_end)
        }
        augment(&mut nodes, ranges);
        Self { nodes }
    }

    /// Visits all numerically containing ranges, including equal boundaries.
    /// A point query passes the same endpoint twice. No query allocation is needed.
    pub fn containing<'a>(&self, ranges: &'a [LccSelector], start: (&str, f64), end: (&str, f64), mut visit: impl FnMut(&'a LccSelector)) {
        fn search<'a>(nodes: &[Node], ranges: &'a [LccSelector], start: (&str, f64), end: (&str, f64), visit: &mut impl FnMut(&'a LccSelector)) {
            if nodes.is_empty() {
                return;
            }
            let root = &nodes[nodes.len() / 2];
            let first = &ranges[nodes[0].range];
            let last = &ranges[root.max_end];
            if compare_lcc_key(&first.start_letters, first.start_number, start.0, start.1).is_gt() || compare_lcc_key(&last.end_letters, last.end_number, end.0, end.1).is_lt() {
                return;
            }
            let (left, rest) = nodes.split_at(nodes.len() / 2);
            let Some((_, right)) = rest.split_first() else { return };
            search(left, ranges, start, end, visit);
            let range = &ranges[root.range];
            if compare_lcc_key(&range.start_letters, range.start_number, start.0, start.1).is_le() {
                if compare_lcc_key(&range.end_letters, range.end_number, end.0, end.1).is_ge() {
                    visit(range);
                }
                search(right, ranges, start, end, visit);
            }
        }
        search(&self.nodes, ranges, start, end, &mut visit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(id: i64, start: (&str, f64), end: (&str, f64)) -> LccSelector {
        LccSelector { concept_id: id, start_letters: start.0.into(), start_number: start.1, end_letters: end.0.into(), end_number: end.1, start_cutters: Vec::new(), end_cutters: Vec::new() }
    }

    fn check(ranges: &[LccSelector], queries: &[(String, f64)]) {
        let index = IntervalIndex::build(ranges);
        let encoded = bincode::serde::encode_to_vec(&index, bincode::config::standard()).unwrap();
        let (decoded, consumed): (IntervalIndex, _) = bincode::serde::decode_from_slice(&encoded, bincode::config::standard()).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(index, decoded);
        for (i, start) in queries.iter().enumerate() {
            // Include points, short spans, and spans crossing the entire search space.
            for end in queries[i..].iter().step_by(7).chain(queries.last()) {
                let mut actual = Vec::new();
                decoded.containing(ranges, (&start.0, start.1), (&end.0, end.1), |r| actual.push(r.concept_id));
                let mut expected: Vec<_> =
                    ranges.iter().filter(|r| compare_lcc_key(&r.start_letters, r.start_number, &start.0, start.1).is_le() && compare_lcc_key(&r.end_letters, r.end_number, &end.0, end.1).is_ge()).map(|r| r.concept_id).collect();
                actual.sort();
                expected.sort();
                assert_eq!(actual, expected, "{start:?}..{end:?}");
            }
        }
    }

    #[test]
    fn overlapping_nested_equal_and_cross_subclass_ranges_match_linear_search() {
        let mut ranges = vec![range(1, ("Q", 0.0), ("QZ", f64::INFINITY))];
        for id in 2..302 {
            let start = ((id * 37) % 101) as f64 / 2.0;
            let end = start + ((id * 11) % 31) as f64;
            ranges.push(range(id, ("QA", start), ("QA", end)));
        }
        ranges.push(range(302, ("QA", 10.0), ("QC", 20.0)));
        ranges.push(range(303, ("QA", 10.0), ("QC", 20.0)));
        ranges.push(range(304, ("QB", 0.0), ("QB", f64::INFINITY)));
        let queries: Vec<_> = ["P", "Q", "QA", "QB", "QC", "QZ", "R"].into_iter().flat_map(|letters| (0..121).map(move |n| (letters.to_owned(), n as f64 / 2.0))).collect();
        check(&ranges, &queries);
    }


}
