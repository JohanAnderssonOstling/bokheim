use super::*;
use structure::{Run, TagData, TagKind};
use crate::tokenized_runs_tree;
use std::hint::black_box;
use std::time::Instant;

pub(super) fn u(kind: UnitKind, text: &str, _len: usize) -> (UnitKind, String) {
    (kind, text.to_string())
}


pub(super) fn stream(units: Vec<(UnitKind, String)>, text: &str) -> SegmentTree {
    let mut pos = 0usize;
    let units = units
        .into_iter()
        .map(|(kind, unit_text)| {
            let start = text[pos..].find(&unit_text).map(|offset| pos + offset).expect("unit text in stream text");
            let end = start + unit_text.len();
            pos = end;
            TokenUnit::new(kind, start..end)
        })
        .collect();
    structure::parser_api::from_tokenized_parts(text.to_string(), units, Vec::new())
}


pub(super) fn assert_non_overlapping_text_nodes(nodes: &[Node]) {
    let mut previous_end = 0;
    for node in nodes.iter().filter(|node| node.kind() != NodeKind::Tag) {
        assert!(node.start() >= previous_end, "node {:?} overlaps a preceding node", node.span());
        previous_end = node.end();
    }
}


pub(super) fn semantic_signature(nodes: &[Node]) -> Vec<(NodeKind, Vec<i128>, Option<String>)> {
    nodes
        .iter()
        .filter(|node| node.semantic().is_some())
        .map(|node| (node.kind(), node.number_values().to_vec(), node.measurement_fact().map(|fact| fact.unit().to_string())))
        .collect()
}


pub(super) fn first_measurement(text: &str) -> Node {
    parse_nodes(&crate::tokenized_text_tree(text), ParseLanguage::English).into_iter().find(|node| node.kind() == NodeKind::Measurement).expect("measurement")
}

pub(super) fn profile_parse_samples(name: &str, samples: &[&str], make_run: impl Fn(&str) -> Run) {
    let streams = samples.iter().map(|sample| tokenized_runs_tree(vec![make_run(sample)])).collect::<Vec<_>>();
    let iterations = std::env::var("RUSTTRADOS_PARSE_PROFILE_ITERS").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(100_000);
    let start = Instant::now();
    let mut nodes = 0usize;
    let mut checksum = 0usize;
    for i in 0..iterations {
        let stream = black_box(&streams[i % streams.len()]);
        let parsed = parse_nodes(stream, ParseLanguage::Neutral);
        nodes = nodes.wrapping_add(parsed.len());
        checksum = checksum.wrapping_add(parsed.len()).wrapping_add(structure::parser_api::units(stream).len());
        black_box(parsed);
    }
    let elapsed = start.elapsed();
    let total_parses = iterations;
    let parses_per_sec = total_parses as f64 / elapsed.as_secs_f64();
    println!("{name}: {total_parses} parses in {elapsed:?}; {parses_per_sec:.0} parses/s; nodes={nodes}; checksum={checksum}");
    black_box((name, iterations, elapsed, nodes, checksum));
}

pub(super) fn tagged_run(sample: &str) -> Run {
    let mid = sample.len() / 2;
    let split = sample[..mid].char_indices().last().map(|(i, c)| i + c.len_utf8()).unwrap_or(0);
    let mut run = Run::default();
    run.push_text(&sample[..split]);
    run.push_tag(TagData::new("1".to_string(), "g".to_string(), TagKind::Open, "<g id=\"1\">".to_string(), None, None));
    run.push_text(&sample[split..]);
    run.push_tag(TagData::new("1".to_string(), "g".to_string(), TagKind::Close, "</g>".to_string(), None, None));
    run
}
