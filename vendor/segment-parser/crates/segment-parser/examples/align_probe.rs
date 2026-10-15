//! Alignment probe: how well do our `Word` nodes line up 1:1 with nlprule's
//! tokens? Prospective POS tagging attaches an nlprule token's POS tag to one
//! of our `Word` nodes, which only works cleanly when each `Word` span equals
//! exactly one nlprule token span. This runs the check over the real en-US
//! source segments in `testdata/default.sdlxliff` and prints every segment
//! where the two tokenizers disagree, so the alignment strategy is grounded in
//! real data before any POS code is written.
//!
//! Run: `cargo run -p segment-parser --example align_probe`

use std::ops::Range;
use std::path::PathBuf;

use nlprule::Tokenizer;
use segment_parser::{NodeKind, ParseLanguage, SegmentTree};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Recursively collect every `*.sdlxliff` under `dir`.
fn find_sdlxliff(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            find_sdlxliff(&path, out);
        } else if path.extension().is_some_and(|e| e == "sdlxliff") {
            out.push(path);
        }
    }
}

/// nlprule only ships `en`/`de` data and we only have the English tokenizer in
/// testdata, so restrict the probe to English-source files.
fn is_english_source(sdlxliff: &str) -> bool {
    sdlxliff
        .find("source-language=\"")
        .map(|i| &sdlxliff[i + "source-language=\"".len()..])
        .and_then(|s| s.split('"').next())
        .is_some_and(|lang| lang.starts_with("en"))
}

/// Minimal `<source>` text extraction from an sdlxliff — strips inline tags and
/// unescapes XML entities. Good enough to recover the plain segment text for a
/// tokenizer-alignment probe; not a real sdlxliff parser.
fn source_segments(sdlxliff: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = sdlxliff;
    while let Some(open) = rest.find("<source>") {
        let after = &rest[open + "<source>".len()..];
        let Some(close) = after.find("</source>") else { break };
        let inner = &after[..close];
        let text = unescape(&strip_tags(inner));
        let text = text.trim();
        if !text.is_empty() && text.chars().any(|c| c.is_alphabetic()) && !out.iter().any(|s| s == text) {
            out.push(text.to_string());
        }
        rest = &after[close + "</source>".len()..];
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#160;", "\u{00A0}")
}

/// The spans POS annotation should target: each `CompoundWord` as a *whole*
/// (its span equals nlprule's single hyphenated token), plus every standalone
/// `Word` leaf that isn't part of a compound. Measuring at leaf-`Word`
/// granularity instead wrongly splits `dust-free` into `dust`/`free` below the
/// `CompoundWord` node that already spans the whole thing.
fn word_node_spans(tree: &SegmentTree) -> Vec<Range<usize>> {
    let compounds: Vec<Range<usize>> = tree
        .preorder_iter()
        .filter(|node| node.kind() == NodeKind::CompoundWord)
        .map(|node| node.span())
        .collect();
    let mut spans = compounds.clone();
    for node in tree.iter_leaf_nodes().filter(|node| node.kind() == NodeKind::Word) {
        let span = node.span();
        let inside_compound = compounds.iter().any(|c| c.start <= span.start && span.end <= c.end);
        if !inside_compound {
            spans.push(span);
        }
    }
    spans.sort_by_key(|s| s.start);
    spans
}

/// nlprule token spans (absolute byte offsets) + their surface text, skipping
/// the empty sentence-start sentinel.
fn nlprule_tokens(tokenizer: &Tokenizer, text: &str) -> Vec<(String, Range<usize>)> {
    let mut out = Vec::new();
    for sentence in tokenizer.pipe(text) {
        for token in sentence.tokens() {
            let span = token.span().byte().clone();
            if span.start == span.end {
                continue;
            }
            out.push((token.word().as_str().to_string(), span));
        }
    }
    out
}

/// Warm throughput of nlprule tokenization vs. our own parser over the corpus,
/// so we can judge whether per-segment POS tagging is viable. Reports both the
/// aggregate rate and the per-segment latency distribution (the number that
/// matters for tagging one edited segment on the UI thread).
fn throughput(segments: &[String], tokenizer: &Tokenizer, parse_language: ParseLanguage, load_elapsed: std::time::Duration) {
    let iters = 5;
    let total_bytes: usize = segments.iter().map(|s| s.len()).sum();
    let gated = segments.iter().filter(|s| s.split_whitespace().count() > 2).count();

    // Per-segment nlprule latency, collected across all iterations.
    let mut nlprule_latencies: Vec<u128> = Vec::with_capacity(segments.len() * iters);
    let nlprule_start = std::time::Instant::now();
    for _ in 0..iters {
        for seg in segments {
            let t = std::time::Instant::now();
            let n: usize = tokenizer.pipe(seg).map(|s| s.tokens().len()).sum();
            std::hint::black_box(n);
            nlprule_latencies.push(t.elapsed().as_nanos());
        }
    }
    let nlprule_elapsed = nlprule_start.elapsed();

    // Our parser (rebuild + parse) for comparison, same corpus.
    let ours_start = std::time::Instant::now();
    for _ in 0..iters {
        for seg in segments {
            let tree = segment_parser::parse_text_with_language(seg.clone(), parse_language);
            std::hint::black_box(tree.preorder_iter().count());
        }
    }
    let ours_elapsed = ours_start.elapsed();

    nlprule_latencies.sort_unstable();
    let n = nlprule_latencies.len();
    let pct = |p: f64| nlprule_latencies[((n as f64 * p) as usize).min(n - 1)] as f64 / 1000.0; // µs
    let calls = (segments.len() * iters) as f64;

    println!("== throughput ({} segments x{iters} iters, {} KB/pass) ==", segments.len(), total_bytes / 1024);
    println!("  nlprule tokenizer load (one-time):  {:.0} ms", load_elapsed.as_secs_f64() * 1000.0);
    println!("  nlprule pipe():   {:>7.0} seg/s  |  {:>6.2} MB/s  |  per-seg µs p50 {:.1}  p95 {:.1}  p99 {:.1}",
        calls / nlprule_elapsed.as_secs_f64(),
        (total_bytes * iters) as f64 / nlprule_elapsed.as_secs_f64() / 1_000_000.0,
        pct(0.50), pct(0.95), pct(0.99));
    println!("  our parser:       {:>7.0} seg/s  |  {:>6.2} MB/s  (rebuild + parse, for comparison)",
        calls / ours_elapsed.as_secs_f64(),
        (total_bytes * iters) as f64 / ours_elapsed.as_secs_f64() / 1_000_000.0);
    println!("  gating: {gated}/{} segments have >2 words (would run nlprule); {} skipped", segments.len(), segments.len() - gated);
    println!();
}

fn main() {
    let root = repo_root();
    let tokenizer_path = root.join("testdata/nlprule/english/en_tokenizer.bin");

    let mut files = Vec::new();
    find_sdlxliff(&root.join("testdata"), &mut files);
    files.sort();

    // Gather unique English source segments across every English-source file.
    let mut segments: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut english_files = 0usize;
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        if !is_english_source(&text) {
            continue;
        }
        english_files += 1;
        for seg in source_segments(&text) {
            if seen.insert(seg.clone()) {
                segments.push(seg);
            }
        }
    }

    let load_start = std::time::Instant::now();
    let tokenizer = Tokenizer::new(&tokenizer_path).expect("load en tokenizer");
    let load_elapsed = load_start.elapsed();
    let parse_language = ParseLanguage::from_bcp47("en-US");

    throughput(&segments, &tokenizer, parse_language, load_elapsed);

    // Dump nlprule's actual POS-tag inventory over the corpus, so the
    // nlprule-tag -> UPOS mapping is grounded in the tags really emitted.
    if std::env::var_os("DUMP_TAGS").is_some() {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for seg in &segments {
            for sentence in tokenizer.pipe(seg) {
                for token in sentence.tokens() {
                    for data in token.word().tags() {
                        *counts.entry(data.pos().as_str().to_string()).or_default() += 1;
                    }
                }
            }
        }
        let mut rows: Vec<_> = counts.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        println!("== nlprule POS tag inventory -> UPOS ({} distinct) ==", rows.len());
        for (tag, n) in &rows {
            let upos = segment_parser::part_of_speech_from_penn(tag);
            let flag = if upos.is_none() && !matches!(tag.as_str(), "" | "UNKNOWN" | "SENT_START" | "SENT_END") { "  <-- UNMAPPED content tag" } else { "" };
            println!("  {:<10} {:>6}  -> {:?}{flag}", format!("{tag:?}"), n, upos);
        }
        println!();

        // Verify the Folkets/SUC class conversion against the real inventory
        // (extracted from testdata/lexicon/folkets_*). Multi-class entries must
        // resolve as documented.
        println!("== Folkets class -> UPOS (folkets_unambiguous) ==");
        for class in ["nn", "vb", "jj", "ab", "pn", "pp", "kn", "sn", "in", "rg", "ro", "pm", "hp", "ps", "ie", "pc", "hjälpverb", "article", "abbrev", "prefix", "suffix", "latin", "", "nn, abbrev", "jj, pc", "ab, jj", "jj, nn, ab", "nn, vb"] {
            println!("  {:<14} -> {:?}", format!("{class:?}"), segment_parser::unambiguous_folkets_part_of_speech(class));
        }
        println!();
        return;
    }

    let mut total_words = 0usize;
    let mut exact_words = 0usize;
    let mut off_segments = 0usize;
    let mut compound_only_segments = 0usize;

    println!(
        "== alignment probe over {} unique en source segments from {} English-source sdlxliff files ==\n",
        segments.len(),
        english_files,
    );

    for seg in &segments {
        let tree = segment_parser::parse_text_with_language(seg.clone(), parse_language);

        let words = word_node_spans(&tree);
        let tokens = nlprule_tokens(&tokenizer, seg);

        let mut seg_mismatches: Vec<String> = Vec::new();
        let mut all_compound_merge = true;
        for span in &words {
            total_words += 1;
            let overlapping: Vec<&(String, Range<usize>)> = tokens
                .iter()
                .filter(|(_, tok)| tok.start < span.end && span.start < tok.end)
                .collect();
            let exact = matches!(overlapping.as_slice(), [(_, tok)] if *tok == *span);
            if exact {
                exact_words += 1;
                continue;
            }
            // Classify the disagreement direction.
            let kind = match overlapping.as_slice() {
                [(tok_text, tok)] if tok.start <= span.start && tok.end >= span.end => {
                    if tok_text.contains('-') { "merge(hyphen)" } else { "merge" }
                }
                [_] => "shift",
                [] => "orphan",
                _ => "split",
            };
            if !kind.starts_with("merge") {
                all_compound_merge = false;
            }
            let toks: Vec<String> = overlapping.iter().map(|(t, r)| format!("{t:?}@{}..{}", r.start, r.end)).collect();
            seg_mismatches.push(format!("      [{kind}] word {:?}@{}..{} -> nlprule [{}]", &seg[span.clone()], span.start, span.end, toks.join(", ")));
        }

        if !seg_mismatches.is_empty() {
            off_segments += 1;
            if all_compound_merge {
                compound_only_segments += 1;
            }
            println!("  SEGMENT: {seg:?}");
            for m in &seg_mismatches {
                println!("{m}");
            }
            println!();
        }
    }

    println!("== summary ==");
    println!("  Word nodes:                 {exact_words}/{total_words} align 1:1 with an nlprule token");
    println!("  segments off:               {off_segments}/{}", segments.len());
    println!("  ...of which hyphen-only:    {compound_only_segments} (only hyphenated-compound merges)");
}
