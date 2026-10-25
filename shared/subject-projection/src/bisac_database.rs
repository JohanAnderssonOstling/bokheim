//! One file-backed, bounded-cache reference connection per process/worker.
use flate2::read::GzDecoder;
#[cfg(target_arch = "wasm32")]
use rusqlite::backup::StepResult;
use rusqlite::{Connection, OptionalExtension, backup::Backup};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

pub const BISAC_REFERENCE_DIGEST: &str = include_str!(concat!(env!("OUT_DIR"), "/bisac-reference-digest.txt"));
pub const BISAC_REFERENCE_GZIP_DIGEST: &str = include_str!(concat!(env!("OUT_DIR"), "/bisac-reference-gzip-digest.txt"));
const IMAGE_SIZE: usize = include!(concat!(env!("OUT_DIR"), "/bisac-reference-size.rs"));
#[cfg(not(target_arch = "wasm32"))]
const IMAGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/bisac-reference.sqlite3.gz"));
static REFERENCE: OnceLock<Mutex<Option<Connection>>> = OnceLock::new();

fn reference() -> MutexGuard<'static, Option<Connection>> {
    let mutex = REFERENCE.get_or_init(|| Mutex::new(None));
    mutex.lock().unwrap_or_else(|error| {
        // Native callers can reopen from the verified bundle. Browser workers
        // retain their initialized, read-only connection: only the host owns
        // the bundle needed to install a new one there.
        #[cfg(not(target_arch = "wasm32"))]
        let mut reference = error.into_inner();
        #[cfg(target_arch = "wasm32")]
        let reference = error.into_inner();
        #[cfg(not(target_arch = "wasm32"))]
        {
            *reference = None;
        }
        mutex.clear_poison();
        eprintln!("Recovering the BISAC reference after an interrupted query");
        reference
    })
}

fn database_path() -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::env::temp_dir().join(format!("bokheim-bisac-reference-{BISAC_REFERENCE_DIGEST}.sqlite3"))
    }
    #[cfg(target_arch = "wasm32")]
    {
        "bisac-reference.sqlite3".into()
    }
}

fn open_file() -> rusqlite::Result<Connection> {
    let db = Connection::open(database_path())?;
    #[cfg(not(target_arch = "wasm32"))]
    db.busy_timeout(Duration::from_secs(30))?;
    db.set_prepared_statement_cache_capacity(8);
    db.execute_batch("PRAGMA cache_size=-128; PRAGMA mmap_size=0;")?;
    Ok(db)
}

fn matches_revision(db: &Connection) -> rusqlite::Result<bool> {
    let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='reference_meta')", [], |row| row.get(0))?;
    if !exists {
        return Ok(false);
    }
    Ok(db.query_row("SELECT revision FROM reference_meta", [], |row| row.get::<_, String>(0)).optional()?.as_deref() == Some(BISAC_REFERENCE_DIGEST))
}

fn image_is_compressed(image: &[u8]) -> Result<bool, String> {
    let digest = blake3::hash(image).to_hex();
    if digest.as_str() == BISAC_REFERENCE_DIGEST {
        Ok(false)
    } else if digest.as_str() == BISAC_REFERENCE_GZIP_DIGEST {
        Ok(true)
    } else {
        Err("BISAC reference differs from the deployed bundle".into())
    }
}

fn copy_image(image: &[u8], db: &mut Connection) -> Result<(), String> {
    let compressed = image_is_compressed(image)?;
    let mut source = Connection::open_in_memory().map_err(|e| e.to_string())?;
    if compressed {
        // The compressed artifact is verified before allocation. Decode directly
        // into SQLite's fixed-size buffer, avoiding a second expanded image.
        source.deserialize_read_exact("main", GzDecoder::new(image), IMAGE_SIZE, true).map_err(|e| e.to_string())?;
    } else {
        source.deserialize_read_exact("main", image, IMAGE_SIZE, true).map_err(|e| e.to_string())?;
    }
    let backup = Backup::new(&source, db).map_err(|e| e.to_string())?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        backup.run_to_completion(64, Duration::from_millis(1), None).map_err(|e| e.to_string())?;
    }
    #[cfg(target_arch = "wasm32")]
    {
        // This worker is the file's only owner. Copy all pages without a
        // blocking sleep or waiting for another same-worker borrower.
        match backup.step(-1).map_err(|e| e.to_string())? {
            StepResult::Done => {}
            _ => return Err("BISAC reference file is busy during installation".into()),
        }
    }
    drop(backup);
    // Cache the complete artifact identity: compiler/schema changes must also
    // replace a persisted reference, even when the CSV itself is unchanged.
    db.execute("UPDATE reference_meta SET revision=?1", [BISAC_REFERENCE_DIGEST]).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn compressed_and_raw_artifacts_install_identical_databases() {
        let raw = include_bytes!(concat!(env!("OUT_DIR"), "/bisac-reference.sqlite3"));
        let mut compressed_target = Connection::open_in_memory().unwrap();
        let mut raw_target = Connection::open_in_memory().unwrap();
        assert!(image_is_compressed(IMAGE).unwrap());
        assert!(!image_is_compressed(raw).unwrap());
        assert!(IMAGE.len() < raw.len() / 2);
        copy_image(IMAGE, &mut compressed_target).unwrap();
        copy_image(raw, &mut raw_target).unwrap();
        assert_eq!(compressed_target.serialize("main").unwrap().as_ref(), raw_target.serialize("main").unwrap().as_ref());
        assert!(matches_revision(&compressed_target).unwrap());
    }

    #[test]
    fn corrupt_compressed_artifacts_cannot_replace_existing_data() {
        let mut target = Connection::open_in_memory().unwrap();
        target.execute_batch("CREATE TABLE preserved(value TEXT); INSERT INTO preserved VALUES('keep');").unwrap();
        let mut damaged = IMAGE.to_vec();
        damaged[IMAGE.len() / 2] ^= 1;
        assert!(copy_image(&damaged, &mut target).unwrap_err().contains("differs from the deployed bundle"));
        assert!(copy_image(&IMAGE[..IMAGE.len() - 1], &mut target).is_err());
        let preserved: String = target.query_row("SELECT value FROM preserved", [], |row| row.get(0)).unwrap();
        assert_eq!(preserved, "keep");
    }

    #[test]
    fn persisted_reference_tracks_the_complete_bundle_artifact() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut db = Connection::open(file.path()).unwrap();
        copy_image(IMAGE, &mut db).unwrap();
        assert!(matches_revision(&db).unwrap());
        db.execute("UPDATE reference_meta SET revision='obsolete artifact'", []).unwrap();
        assert!(!matches_revision(&db).unwrap());
    }
}

pub fn open_bisac_reference() -> Result<bool, String> {
    let mut reference = reference();
    if reference.is_some() {
        return Ok(true);
    }
    let db = open_file().map_err(|e| e.to_string())?;
    if !matches_revision(&db).map_err(|e| e.to_string())? {
        return Ok(false);
    }
    db.execute_batch("PRAGMA query_only=ON;").map_err(|e| e.to_string())?;
    *reference = Some(db);
    Ok(true)
}

pub fn install_bisac_reference(image: &[u8]) -> Result<(), String> {
    image_is_compressed(image)?;
    let mut reference = reference();
    if reference.is_some() {
        return Ok(());
    }
    let mut db = open_file().map_err(|e| e.to_string())?;
    copy_image(image, &mut db)?;
    db.execute_batch("PRAGMA query_only=ON;").map_err(|e| e.to_string())?;
    *reference = Some(db);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn native_reference() -> Result<Connection, String> {
    let mut db = open_file().map_err(|error| error.to_string())?;
    if !matches_revision(&db).map_err(|error| error.to_string())? {
        copy_image(IMAGE, &mut db)?;
    }
    db.execute_batch("PRAGMA query_only=ON;").map_err(|error| error.to_string())?;
    Ok(db)
}

pub(super) fn with_reference<T>(read: impl FnOnce(Option<&Connection>) -> T) -> T {
    let reference = reference();
    #[cfg(not(target_arch = "wasm32"))]
    let mut reference = reference;
    #[cfg(not(target_arch = "wasm32"))]
    if reference.is_none() {
        match native_reference() {
            Ok(db) => *reference = Some(db),
            Err(error) => eprintln!("BISAC storage is unavailable; using compiled reference nodes: {error}"),
        }
    }
    read(reference.as_ref())
}
