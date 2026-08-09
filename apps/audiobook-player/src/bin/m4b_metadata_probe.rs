use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let root = env::args_os().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let mut files = Vec::new();
    collect_m4b(&root, &mut files);
    files.sort();

    println!("M4B files: {}", files.len());
    for path in files {
        println!("\n=== {} ===", path.display());
        match mp4ameta::Tag::read_from_path(&path) {
            Ok(tag) => {
                if let Ok(metadata) = fs::metadata(&path) {
                    println!("file size: {} bytes", metadata.len());
                }
                print!("{tag}");
            }
            Err(error) => println!("metadata error: {error}"),
        }
    }
}

fn collect_m4b(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_file() {
        if is_m4b(path) {
            files.push(path.to_owned());
        }
        return;
    }
    let Ok(entries) = fs::read_dir(path) else {
        eprintln!("cannot read {}", path.display());
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_m4b(&path, files);
        } else if is_m4b(&path) {
            files.push(path);
        }
    }
}

fn is_m4b(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("m4b"))
}
