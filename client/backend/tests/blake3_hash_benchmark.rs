use blake3::Hasher;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Instant;

const BUF_SIZE: usize = 1024 * 1024;
const RUNS: usize = 5;

fn benchmark_epub_path() -> PathBuf {
    std::env::var_os("BOKHEIM_HASH_BENCH_PATH").map(PathBuf::from).expect("set BOKHEIM_HASH_BENCH_PATH to an EPUB before running this benchmark")
}

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

#[test]
#[ignore = "manual benchmark; set BOKHEIM_HASH_BENCH_PATH to an EPUB"]
fn benchmark_full_file_blake3_hash() {
    let path = benchmark_epub_path();
    let path = path.as_path();
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
