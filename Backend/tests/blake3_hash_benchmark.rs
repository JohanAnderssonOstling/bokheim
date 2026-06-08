use blake3::Hasher;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Path;
use std::process::Command;
use std::time::Instant;
use xxhash_rust::xxh3::Xxh3;

const DEFAULT_EPUB_PATH: &str =
    "/home/johan/Hem/Programmering/RustroverProjects/Html-Prototype/Robert D. Putnam - Bowling Alone_ Revised and Updated_ The Collapse and Revival of American Community (2020, Simon & Schuster) - libgen.li.epub";
const BUF_SIZE: usize = 1024 * 1024;
const RUNS: usize = 5;

fn hash_file_blake3(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut file = File::open(path)?;
    let mut hasher = Hasher::new();
    let mut buf = vec![0_u8; BUF_SIZE];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

fn hash_file_fast_xxh3(path: &Path) -> Result<u64, Box<dyn std::error::Error>> {
    const CHUNK: usize = 16 * 1024;
    let mut file = File::open(path)?;
    let file_size = file.metadata()?.len();
    let mut hasher = Xxh3::new();
    let mut buf = vec![0_u8; CHUNK];

    let n = file.read(&mut buf)?;
    hasher.update(&buf[..n]);

    if file_size > CHUNK as u64 {
        file.seek(SeekFrom::End(-(CHUNK as i64)))?;
        let n = file.read(&mut buf)?;
        hasher.update(&buf[..n]);
    }
    hasher.update(&file_size.to_le_bytes());
    Ok(hasher.digest())
}

#[test]
fn benchmark_full_file_blake3_hash() {
    let path_str = std::env::var("BIBLOS_HASH_BENCH_PATH").ok().or_else(discover_epub_path_with_shell).unwrap_or_else(|| DEFAULT_EPUB_PATH.to_string());
    let path = Path::new(&path_str);
    assert!(path.exists(), "Benchmark file does not exist: {}", path.display());

    let size_bytes = std::fs::metadata(path).expect("failed to stat benchmark file").len();
    let size_mb = size_bytes as f64 / (1024.0 * 1024.0);

    let mut total_secs = 0.0_f64;
    let mut last_hash = String::new();

    println!("BLAKE3 benchmark file: {}", path.display());
    println!("Size: {:.2} MB ({} bytes)", size_mb, size_bytes);

    for run in 1..=RUNS {
        let start = Instant::now();
        let hash = hash_file_blake3(path).expect("blake3 hashing failed");
        let elapsed = start.elapsed().as_secs_f64();
        let throughput = size_mb / elapsed.max(f64::EPSILON);
        total_secs += elapsed;
        println!("run {}: {:.6} s ({:.2} MB/s)", run, elapsed, throughput);
        last_hash = hash;
    }

    let avg = total_secs / RUNS as f64;
    let avg_throughput = size_mb / avg.max(f64::EPSILON);
    println!("avg: {:.6} s ({:.2} MB/s)", avg, avg_throughput);
    println!("blake3: {}", last_hash);
}

#[test]
fn benchmark_fast_xxh3_partial_hash() {
    let path_str = std::env::var("BIBLOS_HASH_BENCH_PATH").ok().or_else(discover_epub_path_with_shell).unwrap_or_else(|| DEFAULT_EPUB_PATH.to_string());
    let path = Path::new(&path_str);
    assert!(path.exists(), "Benchmark file does not exist: {}", path.display());

    let size_bytes = std::fs::metadata(path).expect("failed to stat benchmark file").len();
    let size_mb = size_bytes as f64 / (1024.0 * 1024.0);

    let mut total_secs = 0.0_f64;
    let mut last_hash: u64 = 0;

    println!("Fast xxh3 benchmark file: {}", path.display());
    println!("Size: {:.2} MB ({} bytes)", size_mb, size_bytes);

    for run in 1..=RUNS {
        let start = Instant::now();
        let hash = hash_file_fast_xxh3(path).expect("xxh3 hashing failed");
        let elapsed = start.elapsed().as_secs_f64();
        let throughput = size_mb / elapsed.max(f64::EPSILON);
        total_secs += elapsed;
        println!("run {}: {:.6} s ({:.2} MB/s)", run, elapsed, throughput);
        last_hash = hash;
    }

    let avg = total_secs / RUNS as f64;
    let avg_throughput = size_mb / avg.max(f64::EPSILON);
    println!("avg: {:.6} s ({:.2} MB/s)", avg, avg_throughput);
    println!("xxh3_fast: {}", last_hash);
}

fn discover_epub_path_with_shell() -> Option<String> {
    let output = Command::new("bash").arg("-lc").arg("find /home/johan/Hem/Programmering/RustroverProjects/Html-Prototype -type f -iname \"*.epub\" | head -n 1").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8(output.stdout).ok()?;
    let path = s.trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}
