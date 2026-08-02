use html_renderer::HTMLRenderer;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn is_html(path: &Path) -> bool {
    matches!(path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_ascii_lowercase()).as_deref(), Some("html") | Some("htm") | Some("xhtml"))
}

fn collect_html_files(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if is_html(path) {
            out.push(path.to_path_buf());
        }
        return;
    }

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            collect_html_files(&entry.path(), out);
        }
    }
}

fn format_bytes(bytes: usize) -> String {
    let kib = bytes as f64 / 1024.0;
    let mib = kib / 1024.0;
    if mib >= 1.0 {
        format!("{mib:.2} MiB")
    } else if kib >= 1.0 {
        format!("{kib:.2} KiB")
    } else {
        format!("{bytes} B")
    }
}

fn main() {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("TestData"));
    let mut files = Vec::new();
    collect_html_files(&root, &mut files);
    files.sort();

    if files.is_empty() {
        eprintln!("No HTML files found under {}", root.display());
        std::process::exit(1);
    }

    let mut aggregate: HashMap<String, usize> = HashMap::new();
    let mut heaviest: Option<(PathBuf, usize, Vec<(String, usize)>)> = None;
    let mut processed = 0usize;

    for path in files {
        let result = std::panic::catch_unwind(|| HTMLRenderer::from_source_path(path.clone()));
        let renderer = match result {
            Ok(renderer) => renderer,
            Err(_) => {
                eprintln!("skip {}: renderer panicked", path.display());
                continue;
            }
        };

        let mut report = renderer.memory_usage_report();
        report.sort_by_bytes_desc();
        let total = report.total_bytes();
        processed += 1;

        for entry in &report.entries {
            *aggregate.entry(entry.label.clone()).or_insert(0) += entry.bytes;
        }

        let top_entries: Vec<(String, usize)> = report.entries.iter().take(10).map(|entry| (entry.label.clone(), entry.bytes)).collect();
        if heaviest.as_ref().map(|(_, bytes, _)| total > *bytes).unwrap_or(true) {
            heaviest = Some((path.clone(), total, top_entries));
        }
    }

    let mut aggregate_entries: Vec<(String, usize)> = aggregate.into_iter().collect();
    aggregate_entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    println!("Processed {processed} HTML files under {}", root.display());
    if let Some((path, total, top_entries)) = heaviest {
        println!("\nHeaviest document: {} ({})", path.display(), format_bytes(total));
        for (label, bytes) in top_entries {
            println!("  {:>12}  {}", format_bytes(bytes), label);
        }
    }

    println!("\nTop aggregate memory consumers:");
    for (label, bytes) in aggregate_entries.into_iter().take(25) {
        println!("{:>12}  {}", format_bytes(bytes), label);
    }
}
