//! Identity checks and root commits for an explicit library relocation.

use super::recovery::{identity_key, pending_key, storage_identity};
use super::{platform, LibraryEntry, LocalRegistry, RegistryResult};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use sync_common::LibraryId;

fn retired_root_key(id: &LibraryId, identity: &[u8]) -> String {
    format!("library-retired-root:{id}:{}", LibraryId::new_v5(id, identity))
}

impl LocalRegistry {
    pub(super) fn library_root_retired(&mut self, id: &LibraryId, root: &Path) -> RegistryResult<bool> {
        let Some(identity) = storage_identity(root)? else { return Ok(false) };
        Ok(self.read_value(&retired_root_key(id, &identity))?.is_some())
    }

    pub(super) fn validate_move_destination(&mut self, id: &LibraryId, source: &Path, destination: &Path) -> RegistryResult<PathBuf> {
        let entry = self.entry(id)?.ok_or("library not found")?;
        if entry.is_opds() || self.is_detached(id)? {
            return Err("this library cannot be moved".into());
        }
        let destination = destination.canonicalize()?;
        if !destination.is_dir() {
            return Err("select a folder for the library".into());
        }
        if destination.starts_with(source) || source.starts_with(&destination) {
            return Err("choose a separate folder outside the current library".into());
        }
        for registered in self.entries()? {
            let path = Path::new(registered.storage_locator());
            if registered.library_id() != id && (destination.starts_with(path) || path.starts_with(&destination)) {
                return Err("destination overlaps another registered library".into());
            }
            let database = PathBuf::from(self.library_database_locator(registered.library_id(), registered.storage_locator()));
            if database.starts_with(&destination) || destination.starts_with(database.parent().ok_or("invalid library database location")?) {
                return Err("destination overlaps device library state".into());
            }
        }
        let pending: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM pending_local_purge WHERE storage_path=?1)", [destination.to_string_lossy().as_ref()], |row| row.get(0))?;
        if pending {
            return Err("destination is pending library deletion".into());
        }
        platform::validate_move_identity(id, source)?;
        platform::validate_move_identity(id, &destination)?;
        Ok(destination)
    }

    pub(super) fn commit_library_move(&mut self, id: &LibraryId, source: &Path, destination: &Path, source_identity: &str, cleanup: &[u8]) -> RegistryResult<LibraryEntry> {
        let destination = self.validate_move_destination(id, source, destination)?;
        let old = self.entry(id)?.ok_or("library not found")?;
        if Path::new(old.storage_locator()) != source {
            return Err("library location changed during move".into());
        }
        let entry = LibraryEntry::new(*id, old.library_name(), destination.to_string_lossy().into_owned())?;
        let identity = storage_identity(&destination)?.ok_or("destination storage identity is missing")?;
        if storage_identity(source)?.as_deref() != Some(source_identity.as_bytes()) {
            return Err("source storage changed before committing the move".into());
        }
        let tx = self.connection.transaction()?;
        tx.execute("UPDATE library_registry SET storage_path=?2 WHERE library_id=?1", (id.to_string(), entry.storage_locator()))?;
        // An explicit move already verified its files and staged database rebasing.
        // Remember the destination atomically so a crash cannot reopen it through
        // unavailable-root recovery, which deliberately clears local work.
        tx.execute("INSERT INTO app_value(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (identity_key(id), &identity))?;
        tx.execute("DELETE FROM app_value WHERE key=?1", [pending_key(id)])?;
        tx.execute("INSERT INTO app_value(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", (retired_root_key(id, source_identity.as_bytes()), source.to_string_lossy().as_bytes()))?;
        // Copied metadata can share a token. Keep its discovery guard; the
        // registered root is recognized directly before searching candidates.
        if identity != source_identity.as_bytes() {
            tx.execute("DELETE FROM app_value WHERE key=?1", [retired_root_key(id, &identity)])?;
        }
        tx.execute("INSERT INTO app_value(key,value) VALUES(?1,?2)", (format!("library-cleanup:v1:{id}:{}", LibraryId::new_v4()), cleanup))?;
        tx.commit()?;
        // Manifests are repairable metadata. A failure after commit must not
        // report a reverted move or discard its prepared books.
        let _ = platform::prepare_attached_library(*id, entry.library_name(), entry.storage_locator().to_owned());
        Ok(entry)
    }

    pub(super) fn retire_library_move_source(&mut self, id: &LibraryId, source: &Path, source_identity: &str) -> RegistryResult<()> {
        if self.entry(id)?.is_some_and(|current| Path::new(current.storage_locator()) == source) {
            return Err("library still uses its old folder".into());
        }
        platform::validate_move_identity(id, source)?;
        // Retire library manifests, but retain the storage token used by queued
        // historical cleanup. Unrelated files and old databases stay.
        for relative in [".bokheim/library.json", ".bokheim-library.json"] {
            if storage_identity(source)?.as_deref() != Some(source_identity.as_bytes()) {
                return Err("move source storage changed; original metadata was retained".into());
            }
            match fs::remove_file(source.join(relative)) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}
