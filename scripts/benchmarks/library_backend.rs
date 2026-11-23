//! Component measurements; invoked by library_backend.py in fresh processes.
use library_files::AssetStore;
use library_model::RelativeBookPath;
use library_registry::LibraryRegistry;
use serde_json::json;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Cursor, Write};
use std::path::Path;
use std::time::Instant;
use sync_common::BookId;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

fn peak_kib() -> u64 {
    fs::read_to_string("/proc/self/status").unwrap().lines().find_map(|line| line.strip_prefix("VmHWM:")?.split_whitespace().next()?.parse().ok()).unwrap()
}

fn archive(mode: &str, root: &Path) -> Result<()> {
    let start = Instant::now();
    let output = root.join(format!("{mode}.zip"));
    if mode == "buffered" {
        // Reconstruct the former catalog path: full input plus full output.
        let source = fs::read(root.join("source.zip"))?;
        let bytes = audiobook_archive::repack_zip(Cursor::new(source), None)?;
        fs::write(&output, &bytes)?;
    } else {
        let source = BufReader::new(File::open(root.join("source.zip"))?);
        let mut destination = audiobook_archive::repack_zip_into(source, BufWriter::new(File::create(&output)?), None)?;
        destination.flush()?;
    }
    println!("{}", json!({"component":"archive", "mode":mode, "elapsed_ms":start.elapsed().as_secs_f64()*1000.0, "peak_kib":peak_kib(), "output_bytes":fs::metadata(output)?.len()}));
    Ok(())
}

async fn review(mode: &str, root: &Path, count: usize) -> Result<()> {
    let store = AssetStore::open(root.to_str().unwrap())?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(root.join("book-0.epub"))?)?;
    let hash = BookId::from(hasher.finalize());
    let size = fs::metadata(root.join("book-0.epub"))?.len();
    let start = Instant::now();
    let mut present = 0;
    for i in 0..count {
        let paths = vec![RelativeBookPath::parse(&format!("book-{i}.epub"))?];
        let found = if mode == "checksum" { store.verified_book_reader_with_checksum(hash, paths, hash.content_hash()).await?.is_some() } else { store.book_present_for_review(hash, paths, size).await? };
        present += usize::from(found);
    }
    println!("{}", json!({"component":"review", "mode":mode, "count":count, "present":present, "bytes_per_book":size, "elapsed_ms":start.elapsed().as_secs_f64()*1000.0, "peak_kib":peak_kib()}));
    Ok(())
}

fn checkpoints(mode: &str, root: &Path, count: usize) -> Result<()> {
    let registry = LibraryRegistry::open(root.to_str().unwrap())?;
    let start = Instant::now();
    let mut members = Vec::new();
    let mut submitted_bytes = 0;
    for i in 0..count {
        let member = json!({"source":format!("{:064x}", i), "directory":"00000000-0000-0000-0000-000000000000", "destination":format!("{:064x}", i+count)});
        if mode == "whole" {
            members.push(member);
            let bytes = serde_json::to_vec(&json!({"progress":{"books":members}}))?;
            submitted_bytes += bytes.len();
            registry.write_value("copy", &bytes)?;
        } else {
            let bytes = serde_json::to_vec(&member)?;
            submitted_bytes += bytes.len();
            registry.write_value_member("copy", &format!("{i}:root"), &bytes)?;
        }
    }
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    let saved = if mode == "whole" { members.len() } else { registry.read_value_members("copy")?.len() };
    println!("{}", json!({"component":"checkpoint", "mode":mode, "count":count, "saved":saved, "elapsed_ms":elapsed, "submitted_bytes":submitted_bytes,"peak_kib":peak_kib()}));
    registry.remove_value("copy")?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let root = Path::new(&args[3]);
    match args[1].as_str() {
        "archive" => archive(&args[2], root),
        "review" => tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(review(&args[2], root, args[4].parse()?)),
        "checkpoint" => checkpoints(&args[2], root, args[4].parse()?),
        _ => Err(io::Error::other("unknown measurement").into()),
    }
}
