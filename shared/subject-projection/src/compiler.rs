//! Host-side compilation. Clients only validate and open its immutable output.
use crate::{installed, matcher_storage, runtime_format};
use rusqlite::{Connection, OpenFlags, backup::Backup};
use std::path::Path;
use subject_taxonomy::{runtime_index::build_runtime_with_subclasses, taxonomy_reader};

pub fn compile(source: &Path, destination: &Path, release: u64, previous: Option<&Path>) -> Result<(), String> {
    if release > 1 && previous.is_none() {
        return Err("supply the previous release to validate retained identities and redirects".into());
    }
    if release == 0 || destination.exists() {
        return Err("use a positive release ID and a new output path".into());
    }
    let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let concepts = taxonomy_reader::read_unified_taxonomy_connection(&source, "taxonomy source")?;
    let runtime = build_runtime_with_subclasses(concepts, taxonomy_reader::read_lcc_subclasses(&source)?)?;
    let mut db = Connection::open_in_memory().map_err(|e| e.to_string())?;
    {
        let backup = Backup::new(&source, &mut db).map_err(|e| e.to_string())?;
        backup.run_to_completion(256, std::time::Duration::ZERO, None).map_err(|e| e.to_string())?;
    }
    db.execute("UPDATE taxonomy_meta SET value=?1 WHERE key='release_id'", [release.to_string()]).map_err(|e| e.to_string())?;
    db.execute_batch(include_str!("route_storage.sql")).map_err(|e| e.to_string())?;
    matcher_storage::prepare(&db, &runtime)?;
    runtime_format::finish(&db)?;
    if let Some(previous) = previous {
        let previous_metadata = installed::metadata(previous).map_err(|e| e.to_string())?;
        installed::validate(previous, previous_metadata).map_err(|e| e.to_string())?;
        let old = installed::open_read_only(previous).map_err(|e| e.to_string())?;
        let old_release = previous_metadata.0;
        if release <= old_release {
            return Err("taxonomy releases must increase".into());
        }
        let mut ids = old.prepare("SELECT concept_id FROM concept UNION SELECT CAST(substr(path,9) AS INTEGER) FROM taxonomy_path_alias WHERE path GLOB 'concept:[0-9]*'").map_err(|e| e.to_string())?;
        for id in ids.query_map([], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())? {
            let id = id.map_err(|e| e.to_string())?;
            let retained: bool = db
                .query_row("SELECT EXISTS(SELECT 1 FROM concept WHERE concept_id=?1) OR EXISTS(SELECT 1 FROM taxonomy_path_alias a JOIN concept c USING(concept_id) WHERE a.path=?2)", rusqlite::params![id, format!("concept:{id}")], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            if !retained {
                return Err(format!("retired concept {id} needs a redirect before publication"));
            }
        }
    }
    // Validate the closed private image before exposing an immutable publication.
    let image = db.serialize("main").map_err(|e| e.to_string())?;
    installed::validate_image(&image, (release, runtime_format::FORMAT_VERSION)).map_err(|e| e.to_string())?;
    let temporary = destination.with_extension(format!("{}.partial", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
    use std::io::Write;
    let result = (|| {
        file.write_all(&image).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        // Linking publishes atomically and never replaces an existing immutable release.
        std::fs::hard_link(&temporary, destination).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        std::fs::File::open(destination.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."))).and_then(|dir| dir.sync_all()).map_err(|e| e.to_string())?;
        Ok(())
    })();
    drop(file);
    let _ = std::fs::remove_file(temporary);
    result
}
