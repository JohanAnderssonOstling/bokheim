//! A claimed native import source. Only private staging or the published
//! inode is rolled back; the original pathname is never unlinked.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use client_platform_native::file_fingerprint::file_identity;
use client_platform_native::filesystem::{rename_noclobber, sync_directory};
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::filesystem::{fingerprint, rename_into_directory_with_intent};

#[derive(Deserialize, Serialize)]
struct Intent {
    source: PathBuf,
    staged: PathBuf,
    destination: Option<PathBuf>,
    #[serde(default)]
    identity: Option<[u64; 2]>,
}

/// Cancellation or an error restores the source without replacing a file
/// created at its old pathname. If restoration fails, the data and a recovery
/// manifest remain in the private directory named by the error/log.
pub struct ImportMove {
    intent: Intent,
    directory: PathBuf,
    published: Option<PathBuf>,
    published_fingerprint: Option<u64>,
    committed: bool,
    lease: Option<File>,
    registration: Option<PathBuf>,
}

impl ImportMove {
    pub fn claim(source: &Path, destination_directory: &Path) -> io::Result<Self> {
        Self::claim_inner(source, destination_directory, None)
    }

    /// Register before the source rename so normal startup only visits pending
    /// journals, including claims in nested destination folders or mounts.
    pub fn claim_in_library(source: &Path, destination_directory: &Path, root: &Path) -> io::Result<Self> {
        Self::claim_inner(source, destination_directory, Some(root))
    }

    fn claim_inner(source: &Path, destination_directory: &Path, root: Option<&Path>) -> io::Result<Self> {
        if !fs::symlink_metadata(source)?.is_file() {
            return Err(io::Error::other("only regular files can be moved into a library"));
        }
        let source = source.canonicalize()?;
        // Keep staging and publication paths in the same canonical namespace,
        // including the verbatim drive prefix used by Windows canonicalize.
        let destination_directory = destination_directory.canonicalize()?;
        let name = source.file_name().ok_or_else(|| io::Error::other("source has no filename"))?;
        let directory = tempfile::Builder::new().prefix(".bokheim-import-move-").tempdir_in(&destination_directory)?;
        let intent = Intent { source: source.clone(), staged: directory.path().join(name), destination: None, identity: Some(file_identity(&source)?) };
        let lease = OpenOptions::new().read(true).write(true).create_new(true).open(directory.path().join("recovery.lock"))?;
        FileExt::try_lock_exclusive(&lease)?;
        let mut claimed = Self { intent, directory: directory.keep(), published: None, published_fingerprint: None, committed: false, lease: Some(lease), registration: None };
        claimed.write_intent()?;
        // Persist the private directory entry before removing the source name.
        sync_directory(claimed.directory.parent().unwrap())?;
        if let Some(root) = root {
            claimed.registration = Some(register_move(&root.canonicalize()?, &claimed.directory.canonicalize()?)?);
        }
        // EXDEV is deliberately returned with the source untouched. Copy mode
        // remains available when the two locations are on different volumes.
        rename_noclobber(&source, &claimed.intent.staged)?;
        sync_directory(source.parent().unwrap())?;
        sync_directory(&claimed.directory)?;
        // File identity survives rename. Rewriting the same journal would add
        // two flushes without adding recovery information.
        if Some(file_identity(claimed.path())?) != claimed.intent.identity {
            return Err(io::Error::other("source was replaced while claiming the move"));
        }
        Ok(claimed)
    }

    pub fn path(&self) -> &Path {
        &self.intent.staged
    }

    pub fn publish(&mut self, directory: &Path, occupied: &[String]) -> io::Result<PathBuf> {
        let staged = self.intent.staged.clone();
        let result = rename_into_directory_with_intent(&staged, directory, occupied, |destination| {
            self.intent.destination = Some(destination.to_owned());
            self.write_intent()
        })?;
        // An intent can name a candidate whose rename failed. Only roll back
        // a destination after a successful publication.
        self.published = Some(result.clone());
        self.published_fingerprint = Some(fingerprint(&result)?);
        // Windows requires write access for FlushFileBuffers; Unix can flush
        // a read-only handle, preserving support for read-only source files.
        OpenOptions::new().read(true).write(cfg!(windows)).open(&result)?.sync_all()?;
        sync_directory(directory)?;
        sync_directory(&self.directory)?;
        Ok(result)
    }

    fn write_intent(&mut self) -> io::Result<()> {
        let path = self.directory.join("recovery.json");
        let (mut file, created) = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => (file, true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (OpenOptions::new().append(true).open(path)?, false),
            Err(error) => return Err(error),
        };
        // Append a complete intent before each rename. Only initial creation
        // changes a directory entry; subsequent intents need a file flush.
        serde_json::to_writer(&mut file, &self.intent).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        if created {
            sync_directory(&self.directory)?;
        }
        Ok(())
    }

    pub fn rollback(&mut self) -> io::Result<()> {
        let current = if self.intent.staged.try_exists()? {
            Some(self.intent.staged.as_path())
        } else if let Some(path) = &self.published {
            if self.published_fingerprint != Some(fingerprint(path)?) {
                return Err(io::Error::other(format!("published file changed; recover the moved book using {}", self.directory.join("recovery.json").display())));
            }
            Some(path.as_path())
        } else {
            None
        };
        if let Some(current) = current {
            rename_noclobber(current, &self.intent.source).map_err(|error| io::Error::new(error.kind(), format!("could not restore moved source; recover the book using {}: {error}", self.directory.join("recovery.json").display())))?;
            sync_directory(self.intent.source.parent().unwrap())?;
        }
        self.committed = true;
        self.lease.take();
        fs::remove_dir_all(&self.directory)?;
        sync_directory(self.directory.parent().unwrap())?;
        self.remove_registration()
    }

    /// Disable rollback synchronously at the database commit point, before
    /// scheduling cleanup on a worker that may outlive the import future.
    pub fn mark_committed(&mut self) {
        self.committed = true;
    }

    fn remove_registration(&mut self) -> io::Result<()> {
        if let Some(entry) = self.registration.take() {
            remove_registration(&entry)?;
        }
        Ok(())
    }

    /// Called only after the library transaction commits. A duplicate import
    /// can leave its redundant source in private staging; it is safe to retire
    /// that owned path without touching anything at the old source pathname.
    pub fn commit(mut self) -> io::Result<()> {
        self.committed = true;
        self.lease.take();
        fs::remove_dir_all(&self.directory).map_err(|error| io::Error::new(error.kind(), format!("could not clean private import staging {}: {error}", self.directory.display())))?;
        sync_directory(self.directory.parent().unwrap())?;
        self.remove_registration()
    }
}

fn registry(root: &Path) -> io::Result<PathBuf> {
    let metadata = root.join(crate::APP_HIDDEN_DIR);
    let registry = metadata.join("import-moves");
    for path in [&metadata, &registry] {
        match fs::create_dir(path) {
            Ok(()) => sync_directory(path.parent().unwrap())?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_dir() => {}
            Err(error) => return Err(error),
        }
    }
    Ok(registry)
}

fn register_move(root: &Path, directory: &Path) -> io::Result<PathBuf> {
    let relative = directory.strip_prefix(root).map_err(io::Error::other)?;
    let registry = registry(root)?;
    let temporary = tempfile::NamedTempFile::new_in(&registry)?;
    serde_json::to_writer(temporary.as_file(), relative).map_err(io::Error::other)?;
    temporary.as_file().sync_all()?;
    let entry = registry.join(format!("{}.json", uuid::Uuid::new_v4()));
    temporary.persist(&entry).map_err(|error| error.error)?;
    sync_directory(&registry)?;
    Ok(entry)
}

fn remove_registration(entry: &Path) -> io::Result<()> {
    match fs::remove_file(entry) {
        // A stale registration is harmless: recovery removes it after finding
        // that the durably retired private directory no longer exists.
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Restore abandoned claims before library scanning starts. A per-journal OS
/// lock keeps independently opened sessions from recovering active imports.
/// Published files are retained only when the database confirms their commit.
pub fn recover_import_moves(root: &Path, mut is_committed: impl FnMut(&Path) -> io::Result<bool>) -> io::Result<Vec<String>> {
    let root = root.canonicalize()?;
    let mut unresolved = Vec::new();
    let registry = registry(&root)?;
    let mut missing = Vec::new();
    for entry in fs::read_dir(&registry)? {
        let entry = entry?;
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let result = (|| -> io::Result<()> {
            let relative: PathBuf = serde_json::from_reader(File::open(entry.path())?).map_err(io::Error::other)?;
            if relative.as_os_str().is_empty() || !relative.components().all(|component| matches!(component, Component::Normal(_))) || !relative.file_name().is_some_and(|name| name.to_string_lossy().starts_with(".bokheim-import-move-")) {
                return Err(io::Error::other("invalid registered move path"));
            }
            let directory = root.join(relative);
            if directory.try_exists()? {
                if !directory.canonicalize()?.starts_with(&root) {
                    return Err(io::Error::other("registered move escaped the library"));
                }
                recover_one(&directory, &mut is_committed)?;
                if !directory.try_exists()? {
                    remove_registration(&entry.path())?;
                }
            } else {
                // A destination folder may have been renamed while a worker
                // owned the claim. Locate its journal once before retiring the
                // old pointer; a completed cleanup can also leave a stale one.
                missing.push(entry.path());
            }
            Ok(())
        })();
        if let Err(error) = result {
            unresolved.push(format!("{}: {error}", entry.path().display()));
        }
    }
    // Upgrade old libraries once. Every new claim is registered durably before
    // rename; unresolved and active legacy claims are registered before marking
    // the scan complete. A traversal failure leaves the upgrade pending.
    let marker = registry.join("legacy-scan-complete");
    if marker.try_exists()? && missing.is_empty() {
        return Ok(unresolved);
    }
    let mut scanned = true;
    let mut entries = walkdir::WalkDir::new(&root).follow_links(false).into_iter();
    while let Some(entry) = entries.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                scanned = false;
                unresolved.push(error.to_string());
                continue;
            }
        };
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.starts_with(".bokheim-import-move-") {
            entries.skip_current_dir();
            let result = (|| -> io::Result<()> {
                let registration = register_move(&root, entry.path())?;
                if entry.path().try_exists()? {
                    recover_one(entry.path(), &mut is_committed)?;
                }
                if !entry.path().try_exists()? {
                    remove_registration(&registration)?;
                }
                Ok(())
            })();
            if let Err(error) = result {
                scanned = false;
                unresolved.push(format!("{}: {error}", entry.path().join("recovery.json").display()));
            }
        } else if name.starts_with('.') {
            entries.skip_current_dir();
        }
    }
    if scanned {
        let file = OpenOptions::new().write(true).create(true).truncate(false).open(marker)?;
        file.sync_all()?;
        sync_directory(&registry)?;
        for entry in missing {
            remove_registration(&entry)?;
        }
    }
    Ok(unresolved)
}

fn recover_one(directory: &Path, is_committed: &mut impl FnMut(&Path) -> io::Result<bool>) -> io::Result<()> {
    let lease = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(directory.join("recovery.lock"))?;
    match FileExt::try_lock_exclusive(&lease) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::WouldBlock || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => return Ok(()),
        Err(error) => return Err(error),
    }
    let mut intent: Intent = match File::open(directory.join("recovery.json")) {
        Ok(file) => {
            let mut latest = None;
            for record in serde_json::Deserializer::from_reader(file).into_iter::<Intent>() {
                match record {
                    Ok(intent) => latest = Some(intent),
                    // A crash before the append is flushed cannot have run
                    // its rename. Recover using the previous durable intent.
                    Err(error) if error.is_eof() && latest.is_some() => break,
                    Err(error) => return Err(io::Error::other(error)),
                }
            }
            latest.ok_or_else(|| io::Error::other("empty move recovery journal"))?
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()), // Claim did not yet rename a source.
        Err(error) => return Err(error),
    };
    let parent = directory.parent().ok_or_else(|| io::Error::other("missing move directory parent"))?;
    let old_directory = intent.staged.parent().ok_or_else(|| io::Error::other("invalid staged move path"))?;
    if old_directory != directory && old_directory.file_name() == directory.file_name() && intent.destination.as_ref().is_none_or(|path| path.parent() == old_directory.parent()) {
        intent.destination = intent.destination.as_ref().and_then(|path| path.file_name()).map(|name| parent.join(name));
        intent.staged = directory.join(intent.staged.file_name().ok_or_else(|| io::Error::other("missing staged filename"))?);
    }
    if !intent.source.is_absolute() || intent.source.file_name().is_none() || intent.staged != directory.join(intent.source.file_name().unwrap()) || intent.destination.as_ref().is_some_and(|path| path.parent() != Some(parent)) {
        return Err(io::Error::other("invalid move recovery paths; files retained"));
    }
    let restore = if intent.staged.try_exists()? {
        Some(intent.staged.as_path())
    } else if let Some(destination) = intent.destination.as_deref().filter(|path| path.exists()) {
        // The candidate was journaled before rename. Prove that publication
        // actually succeeded before touching it, including the crash window
        // before its fingerprint could be recorded.
        if intent.identity.is_none() || Some(file_identity(destination)?) != intent.identity {
            return Err(io::Error::other("published file identity changed; files retained"));
        }
        if is_committed(destination)? {
            None
        } else {
            Some(destination)
        }
    } else if intent.source.try_exists()? && (intent.identity.is_none() || Some(file_identity(&intent.source)?) == intent.identity) {
        None // Interrupted before claim or after successful restoration.
    } else {
        return Err(io::Error::other("moved file could not be located; journal retained"));
    };
    if let Some(path) = restore {
        rename_noclobber(path, &intent.source)?;
        sync_directory(intent.source.parent().unwrap())?;
        sync_directory(path.parent().unwrap())?;
    }
    drop(lease);
    fs::remove_dir_all(directory)?;
    sync_directory(parent)
}

impl Drop for ImportMove {
    fn drop(&mut self) {
        if !self.committed {
            if let Err(error) = self.rollback() {
                log::error!("import move rollback: {error}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abandon_claim(mut claim: ImportMove) {
        claim.committed = true;
        // Other parallel tests spawn crash children. Explicit unlock avoids a
        // transient inherited descriptor making this synthetic crash look live.
        FileExt::unlock(claim.lease.as_ref().unwrap()).unwrap();
    }

    #[test]
    fn indexed_recovery_finds_claims_after_the_legacy_scan_and_root_relocation() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir_all(library.join("nested")).unwrap();
        assert!(recover_import_moves(&library, |_| Ok(false)).unwrap().is_empty());
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claim = ImportMove::claim_in_library(&source, &library.join("nested"), &library).unwrap();
        claim.publish(&library.join("nested"), &[]).unwrap();
        // Simulate a crash without running rollback, while releasing the lock.
        abandon_claim(claim);
        let relocated = root.path().join("relocated");
        fs::rename(&library, &relocated).unwrap();
        assert!(recover_import_moves(&relocated, |_| Ok(false)).unwrap().is_empty());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert_eq!(fs::read_dir(relocated.join("nested")).unwrap().count(), 0);
        assert!(recover_import_moves(&relocated, |_| Ok(false)).unwrap().is_empty());
    }

    #[test]
    fn indexed_recovery_does_not_walk_unregistered_subtrees_after_migration() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir_all(library.join("nested")).unwrap();
        recover_import_moves(&library, |_| Ok(false)).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        // A legacy claim created after migration is deliberately unindexed.
        // Its continued presence proves normal startup did not walk the tree.
        let claim = ImportMove::claim(&source, &library.join("nested")).unwrap();
        let staged = claim.path().to_owned();
        abandon_claim(claim);
        assert!(recover_import_moves(&library, |_| Ok(false)).unwrap().is_empty());
        assert!(staged.exists());
        fs::rename(staged, source).unwrap();
    }

    #[test]
    fn indexed_recovery_locates_a_renamed_destination_folder() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        let destination = library.join("nested");
        fs::create_dir_all(&destination).unwrap();
        recover_import_moves(&library, |_| Ok(false)).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claim = ImportMove::claim_in_library(&source, &destination, &library).unwrap();
        claim.publish(&destination, &[]).unwrap();
        abandon_claim(claim);
        fs::rename(&destination, library.join("renamed")).unwrap();
        assert!(recover_import_moves(&library, |_| Ok(false)).unwrap().is_empty());
        assert_eq!(fs::read(source).unwrap(), b"original");
        assert_eq!(fs::read_dir(library.join("renamed")).unwrap().count(), 0);
    }

    #[test]
    fn recovery_uses_the_last_complete_intent_after_an_interrupted_append() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir(&library).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claim = ImportMove::claim_in_library(&source, &library, &library).unwrap();
        claim.publish(&library, &[]).unwrap();
        let mut journal = OpenOptions::new().append(true).open(claim.directory.join("recovery.json")).unwrap();
        journal.write_all(b"{\"source\":").unwrap();
        journal.sync_all().unwrap();
        abandon_claim(claim);
        assert!(recover_import_moves(&library, |_| Ok(false)).unwrap().is_empty());
        assert_eq!(fs::read(source).unwrap(), b"original");
    }

    #[test]
    fn rollback_restores_source_after_validation_failure() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("library");
        fs::create_dir(&destination).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        {
            let claimed = ImportMove::claim(&source, &destination).unwrap();
            assert_eq!(fs::read(claimed.path()).unwrap(), b"original");
        }
        assert_eq!(fs::read(source).unwrap(), b"original");
    }

    #[test]
    fn commit_never_deletes_a_replacement_at_the_old_source_path() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("library");
        fs::create_dir(&destination).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claimed = ImportMove::claim(&source, &destination).unwrap();
        fs::write(&source, b"replacement").unwrap();
        let published = claimed.publish(&destination, &[]).unwrap();
        claimed.commit().unwrap();
        assert_eq!(fs::read(source).unwrap(), b"replacement");
        assert_eq!(fs::read(published).unwrap(), b"original");
    }

    #[test]
    fn failed_rollback_preserves_both_files_and_the_recovery_manifest() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("library");
        fs::create_dir(&destination).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claimed = ImportMove::claim(&source, &destination).unwrap();
        let recovery = claimed.directory.join("recovery.json");
        let staged = claimed.path().to_owned();
        fs::write(&source, b"replacement").unwrap();
        assert!(claimed.rollback().is_err());
        drop(claimed);
        assert_eq!(fs::read(source).unwrap(), b"replacement");
        assert_eq!(fs::read(staged).unwrap(), b"original");
        assert!(recovery.is_file());
    }

    #[test]
    fn rollback_after_publication_restores_source() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("library");
        fs::create_dir(&destination).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claimed = ImportMove::claim(&source, &destination).unwrap();
        let published = claimed.publish(&destination, &[]).unwrap();
        claimed.rollback().unwrap();
        assert_eq!(fs::read(source).unwrap(), b"original");
        assert!(!published.exists());
    }

    #[test]
    fn rollback_does_not_move_a_replacement_at_the_published_path() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("library");
        fs::create_dir(&destination).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let mut claimed = ImportMove::claim(&source, &destination).unwrap();
        let published = claimed.publish(&destination, &[]).unwrap();
        let original = destination.join("externally-moved.epub");
        fs::rename(&published, &original).unwrap();
        fs::write(&published, b"replacement").unwrap();
        assert!(claimed.rollback().is_err());
        assert_eq!(fs::read(published).unwrap(), b"replacement");
        assert_eq!(fs::read(original).unwrap(), b"original");
        assert!(!source.exists());
    }

    // A subprocess exit skips Drop, matching an application crash rather
    // than ordinary cancellation. These env vars only affect this child test.
    #[test]
    fn recovery_crash_child() {
        let Some(root) = std::env::var_os("BOKHEIM_MOVE_CRASH_TEST_ROOT") else { return };
        let root = PathBuf::from(root);
        let destination = root.join("library/nested");
        let mut claim = ImportMove::claim(&root.join("book.epub"), &destination).unwrap();
        if std::env::var_os("BOKHEIM_MOVE_CRASH_TEST_PUBLISH").is_some() {
            claim.publish(&destination, &[]).unwrap();
        }
        std::process::exit(0);
    }

    fn crashed_move(root: &Path, published: bool) -> PathBuf {
        let library = root.join("library");
        fs::create_dir_all(library.join("nested")).unwrap();
        fs::write(root.join("book.epub"), b"original").unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.arg("--exact").arg("import_move::tests::recovery_crash_child").env("BOKHEIM_MOVE_CRASH_TEST_ROOT", root);
        if published {
            child.env("BOKHEIM_MOVE_CRASH_TEST_PUBLISH", "1");
        } else {
            child.env_remove("BOKHEIM_MOVE_CRASH_TEST_PUBLISH");
        }
        let result = child.output().unwrap();
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
        assert!(!root.join("book.epub").exists());
        library
    }

    #[test]
    fn restart_restores_an_unpublished_claim_and_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let library = crashed_move(root.path(), false);
        assert!(recover_import_moves(&library, |_| panic!("not published")).unwrap().is_empty());
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), b"original");
        assert!(recover_import_moves(&library, |_| Ok(false)).unwrap().is_empty());
        assert_eq!(fs::read_dir(library.join("nested")).unwrap().count(), 0);
    }
    #[test]
    fn restart_restores_publication_before_database_commit() {
        let root = tempfile::tempdir().unwrap();
        let library = crashed_move(root.path(), true);
        assert!(recover_import_moves(&library, |_| Ok(false)).unwrap().is_empty());
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), b"original");
        assert!(!library.join("nested/book.epub").exists());
    }
    #[test]
    fn restart_keeps_a_database_committed_publication() {
        let root = tempfile::tempdir().unwrap();
        let library = crashed_move(root.path(), true);
        assert!(recover_import_moves(&library, |_| Ok(true)).unwrap().is_empty());
        assert!(!root.path().join("book.epub").exists());
        assert_eq!(fs::read(library.join("nested/book.epub")).unwrap(), b"original");
        assert_eq!(fs::read_dir(library.join("nested")).unwrap().count(), 1);
    }
    #[test]
    fn restart_never_overwrites_a_replacement_at_the_original_path() {
        let root = tempfile::tempdir().unwrap();
        let library = crashed_move(root.path(), false);
        fs::write(root.path().join("book.epub"), b"replacement").unwrap();
        assert_eq!(recover_import_moves(&library, |_| Ok(false)).unwrap().len(), 1);
        assert_eq!(fs::read(root.path().join("book.epub")).unwrap(), b"replacement");
        let private = fs::read_dir(library.join("nested")).unwrap().next().unwrap().unwrap().path();
        assert_eq!(fs::read(private.join("book.epub")).unwrap(), b"original");
        assert!(private.join("recovery.json").exists());
    }
    #[test]
    fn restart_does_not_touch_a_replacement_at_the_publication_path() {
        let root = tempfile::tempdir().unwrap();
        let library = crashed_move(root.path(), true);
        let published = library.join("nested/book.epub");
        fs::rename(&published, library.join("nested/original.epub")).unwrap();
        fs::write(&published, b"replacement").unwrap();
        assert_eq!(recover_import_moves(&library, |_| Ok(true)).unwrap().len(), 1);
        assert_eq!(fs::read(published).unwrap(), b"replacement");
        assert_eq!(fs::read(library.join("nested/original.epub")).unwrap(), b"original");
    }
    #[test]
    fn restart_skips_a_live_import() {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        fs::create_dir(&library).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let claim = ImportMove::claim(&source, &library).unwrap();
        assert!(recover_import_moves(&library, |_| panic!("active import")).unwrap().is_empty());
        assert!(claim.path().exists());
        assert!(!source.exists());
        drop(claim);
        assert!(source.exists());
    }

    #[cfg(unix)]
    #[test]
    fn move_preserves_the_source_inode() {
        use std::os::unix::fs::MetadataExt;
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("library");
        fs::create_dir(&destination).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"original").unwrap();
        let inode = fs::metadata(&source).unwrap().ino();
        let mut claimed = ImportMove::claim(&source, &destination).unwrap();
        let published = claimed.publish(&destination, &[]).unwrap();
        claimed.commit().unwrap();
        assert_eq!(fs::metadata(published).unwrap().ino(), inode);
    }
}
