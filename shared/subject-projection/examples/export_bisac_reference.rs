use std::path::PathBuf;

fn main() -> std::io::Result<()> {
    let directory = PathBuf::from(std::env::args_os().nth(1).expect("supply the web bundle directory"));
    let digest = include_str!(concat!(env!("OUT_DIR"), "/bisac-reference-gzip-digest.txt"));
    std::fs::write(directory.join(format!("bisac-reference-{digest}.sqlite3.gz")), include_bytes!(concat!(env!("OUT_DIR"), "/bisac-reference.sqlite3.gz")))
}
