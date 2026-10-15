//! Strong compression is paid once at publication; clients store ordinary SQLite.
use flate2::{write::GzEncoder, Compression};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use zstd::stream::Encoder;

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, "compressed snapshot already exists"));
    }
    let temporary = suffixed(path, ".partial");
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all()).and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(super) fn write_sidecars(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let gzip_path = suffixed(path, ".gz");
    let zstd_path = suffixed(path, ".zst");
    if gzip_path.exists() || zstd_path.exists() {
        return Err("compressed snapshot already exists".into());
    }
    let mut gzip = GzEncoder::new(Vec::new(), Compression::best());
    gzip.write_all(bytes).map_err(|error| error.to_string())?;
    let gzip = gzip.finish().map_err(|error| error.to_string())?;
    let mut zstd = Encoder::new(Vec::new(), 19).map_err(|error| error.to_string())?;
    // RFC 9659 limits the zstd HTTP coding to an 8 MiB window.
    zstd.window_log(23).map_err(|error| error.to_string())?;
    zstd.set_pledged_src_size(Some(bytes.len() as u64)).map_err(|error| error.to_string())?;
    zstd.write_all(bytes).map_err(|error| error.to_string())?;
    let zstd = zstd.finish().map_err(|error| error.to_string())?;
    write_atomic(&gzip_path, &gzip).map_err(|error| error.to_string())?;
    if let Err(error) = write_atomic(&zstd_path, &zstd) {
        let _ = fs::remove_file(&gzip_path);
        return Err(error.to_string());
    }
    eprintln!("{}: {} bytes SQLite, {} bytes gzip-9, {} bytes zstd-19", path.display(), bytes.len(), gzip.len(), zstd.len());
    Ok(())
}

pub(super) fn remove_sidecars(path: &Path) {
    let _ = fs::remove_file(suffixed(path, ".gz"));
    let _ = fs::remove_file(suffixed(path, ".zst"));
}
