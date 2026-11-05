//! Recursive library-folder merges with verified, resumable cleanup.
//! Originals remain until the registry commits the prepared destination.

use audiobook_folder::{discover, discover_cover, discover_cue, discover_nfo, folder_hash, PATH_MAPPING_FILE};
use client_platform_native::filesystem::sync_directory;
use content_address::{hash_file, ContentHash, HashingReader};
use library_model::RelativeBookPath;
use library_replica::{file_name_candidates, folder_name_candidates, portable_name_key, project_folder_name};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Error, ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;
use sync_common::{DirId, LibraryId, ROOT_DIR_ID};

#[derive(Clone, Serialize, Deserialize)]
pub struct MovePlan {
    pub source: PathBuf,
    pub destination: PathBuf,
    files: Vec<FileMove>,
    #[serde(default)]
    directories: Vec<DirectoryMove>,
    #[serde(default)]
    whole_tree: bool,
}

#[derive(Clone, Serialize, Deserialize)]
struct DirectoryMove {
    source: PathBuf,
    destination: PathBuf,
    id: Option<DirId>,
}

#[derive(Clone, Serialize, Deserialize)]
struct FileMove {
    source: PathBuf,
    destination: PathBuf,
    checksum: Option<ContentHash>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    modified: Option<SystemTime>,
    // Historical plans treated an audiobook folder as one file snapshot.
    #[serde(default)]
    folder: bool,
    #[serde(default)]
    members: Vec<Member>,
    #[serde(default)]
    cleanup_started: bool,
}
impl FileMove {
    fn checksum(&self) -> io::Result<ContentHash> {
        self.checksum.ok_or_else(|| failure("library file has not been transferred"))
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Member {
    path: PathBuf,
    checksum: ContentHash,
}

struct CreatedMarker {
    relative: PathBuf,
    id: DirId,
}

/// Only newly published files and directory markers belong to rollback.
pub struct MoveTransfer {
    rollback: Option<(MovePlan, LibraryId, String)>,
    markers: Vec<CreatedMarker>,
}
impl MoveTransfer {
    pub fn commit(mut self) {
        self.rollback.take();
    }
}
impl Drop for MoveTransfer {
    fn drop(&mut self) {
        let Some((mut plan, library, identity)) = self.rollback.take() else { return };
        let root = plan.source.clone();
        let check = || {
            if cleanup_root_matches(&root, &identity, library)? {
                Ok(())
            } else {
                Err(failure("destination storage changed; preparation files were retained"))
            }
        };
        let rollback = (|| {
            for marker in self.markers.iter().rev() {
                check()?;
                let path = checked_path(&root, &marker.relative)?;
                if read_directory_id(&path)? != Some(marker.id) {
                    return Err(failure("destination folder identity changed; preparation was retained"));
                }
                fs::remove_file(&path)?;
                sync_directory(path.parent().ok_or_else(|| failure("folder marker has no parent"))?)?;
            }
            plan.cleanup_originals(&check, |_| Ok(()))
        })();
        if let Err(error) = rollback {
            log::warn!("could not remove all uncommitted library preparation files: {error}");
        }
    }
}

pub fn stored_relative_path(path: &Path) -> io::Result<String> {
    let components = path
        .components()
        .map(|component| {
            let Component::Normal(name) = component else { return Err(failure("invalid relative library path")) };
            name.to_str().ok_or_else(|| failure("library paths must contain valid Unicode"))
        })
        .collect::<io::Result<Vec<_>>>()?;
    RelativeBookPath::parse(&components.join("/")).map(|path| path.to_string()).map_err(|error| failure(error.to_string()))
}

/// Historical cleanup is valid only for the original store. A retired root
/// may have no library manifests, but a different library may never be touched.
pub fn cleanup_root_matches(root: &Path, identity: &str, library: LibraryId) -> io::Result<bool> {
    let marker = checked_path(root, Path::new(".bokheim/storage-identity"))?;
    let current = match fs::read_to_string(marker) {
        Ok(current) => current,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if current != identity {
        return Ok(false);
    }
    #[derive(Deserialize)]
    struct Owner {
        library_id: LibraryId,
    }
    for relative in [".bokheim/library.json", ".bokheim-library.json"] {
        let path = checked_path(root, Path::new(relative))?;
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        let owner: Owner = serde_json::from_slice(&bytes).map_err(|error| failure(error.to_string()))?;
        if owner.library_id != library {
            return Ok(false);
        }
    }
    Ok(true)
}

fn original_members_present(root: &Path, book: &FileMove) -> io::Result<bool> {
    let source = checked_path(root, &book.source)?;
    if !source.try_exists()? {
        return Ok(false);
    }
    if !book.folder {
        return Ok(true);
    }
    for member in &book.members {
        if checked_path(&source, &member.path)?.try_exists()? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn failure(message: impl Into<String>) -> Error {
    Error::other(message.into())
}

/// Resolve only ordinary paths below a canonical root, refusing symlink traversal.
pub fn checked_path(root: &Path, relative: &Path) -> io::Result<PathBuf> {
    if root.canonicalize()? != root {
        return Err(failure("library root changed during move"));
    }
    let mut path = root.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else { return Err(failure("invalid relative library path")) };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_symlink() => return Err(failure(format!("library move refuses symbolic links: {}", path.display()))),
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    if path == root {
        return Err(failure("book path cannot be the library root"));
    }
    Ok(path)
}

pub fn book_checksum(path: &Path) -> io::Result<ContentHash> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        hash_file(&mut File::open(path)?)
    } else if metadata.is_dir() {
        // Hashing the reproducible archive matches scanner/download identity.
        let mut walk = crate::filesystem::walk(path);
        while let Some(entry) = walk.next() {
            if entry?.is_symlink() {
                return Err(failure("audiobook folders containing symbolic links cannot be moved"));
            }
        }
        folder_hash(path).map(ContentHash::from)
    } else {
        Err(failure("only ordinary book files and audiobook folders can be moved"))
    }
}

fn members(path: &Path, folder: bool) -> io::Result<Vec<Member>> {
    if !folder {
        return Ok(Vec::new());
    }
    let mut paths = discover(path)?.into_iter().map(|track| track.path).collect::<Vec<_>>();
    paths.extend(discover_cover(path)?.map(|member| member.path));
    paths.extend(discover_cue(path)?.map(|member| member.path));
    paths.extend(discover_nfo(path)?.map(|member| member.path));
    if path.join(PATH_MAPPING_FILE).try_exists()? {
        paths.push(path.join(PATH_MAPPING_FILE));
    }
    paths.into_iter().map(|file| Ok(Member { path: file.strip_prefix(path).map_err(|_| failure("audiobook member is outside its folder"))?.to_owned(), checksum: hash_file(&mut File::open(file)?)? })).collect()
}

impl MovePlan {
    /// Plan every ordinary file and folder using portable folder names and stable identities.
    pub fn prepare(source: PathBuf, destination: PathBuf) -> io::Result<Self> {
        let entries = incoming_entries(&source)?;
        let mut context = MergeContext::new(&destination)?;
        let mut plan = Self { source, destination, files: Vec::new(), directories: Vec::new(), whole_tree: true };
        let mut parents = HashMap::from([(PathBuf::new(), PathBuf::new())]);
        let mut reserved: HashMap<PathBuf, HashSet<String>> = HashMap::new();
        for entry in &entries {
            reserved.entry(entry.relative.parent().unwrap_or(Path::new("")).to_owned()).or_default().insert(entry.name.clone());
        }
        // Every parent is resolved before its descendants. A renamed parent
        // therefore redirects its entire subtree before any files are moved.
        for entry in entries {
            let source_parent = entry.relative.parent().ok_or_else(|| failure("source entry has no parent"))?;
            let target_parent = parents.get(source_parent).ok_or_else(|| failure("source parent was not planned"))?;
            let requested = target_parent.join(&entry.name);
            let known = entry.id.and_then(|id| context.destination_ids.get(&id)).cloned();
            let mut checksum = None;
            let destination = if let Some(known) = known {
                // Reuse a retained identity, including its device-local spelling.
                if known.parent() != Some(target_parent.as_path()) {
                    return Err(failure("destination folder identities have a different hierarchy; originals were retained"));
                }
                if !context.claim(&known) {
                    return Err(failure("multiple folders would share a portable destination path; originals were retained"));
                }
                known
            } else {
                choose_destination(&plan.source, &plan.destination, &requested, &entry, &reserved[source_parent], &mut context, &mut checksum)?
            };
            if entry.directory {
                parents.insert(entry.relative.clone(), destination.clone());
                plan.directories.push(DirectoryMove { source: entry.relative, destination, id: entry.id });
            } else {
                plan.files.push(FileMove { source: entry.relative, destination, checksum, size: Some(entry.size), modified: entry.modified, folder: false, members: Vec::new(), cleanup_started: false });
            }
        }
        Ok(plan)
    }

    /// Resolve a registered placement after any name conflicts were renamed.
    pub fn destination_for(&self, relative: &Path) -> Option<&Path> {
        self.files.iter().find(|file| file.source == relative).map(|file| file.destination.as_path()).or_else(|| self.directories.iter().find(|directory| directory.source == relative).map(|directory| directory.destination.as_path()))
    }

    pub fn transferred_checksum(&self, relative: &Path) -> Option<ContentHash> {
        self.files.iter().find(|file| file.source == relative).and_then(|file| file.checksum)
    }

    pub fn relocated_directories(&self, registered: &HashSet<DirId>) -> io::Result<Vec<(DirId, String)>> {
        self.directories.iter().filter_map(|directory| directory.id.filter(|id| registered.contains(id)).map(|id| Ok((id, stored_relative_path(&directory.destination)?)))).collect()
    }

    pub fn transfer(&mut self, library: LibraryId, destination_identity: String) -> io::Result<MoveTransfer> {
        let mut transfer = MoveTransfer {
            rollback: Some((Self { source: self.destination.clone(), destination: self.destination.clone(), files: Vec::new(), directories: Vec::new(), whole_tree: true }, library, destination_identity)),
            markers: Vec::new(),
        };
        for directory in &self.directories {
            let target = checked_path(&self.destination, &directory.destination)?;
            match fs::create_dir(&target) {
                Ok(()) => transfer.rollback.as_mut().expect("uncommitted transfer").0.directories.push(DirectoryMove { source: directory.destination.clone(), destination: directory.destination.clone(), id: None }),
                Err(error) if error.kind() == ErrorKind::AlreadyExists && target.is_dir() => {}
                Err(error) => return Err(error),
            }
            sync_directory(target.parent().ok_or_else(|| failure("folder has no parent"))?)?;
            if let Some(id) = directory.id {
                if read_directory_id(&checked_path(&self.source, &directory.source.join(".biblos_uuid"))?)? != Some(id) {
                    return Err(failure("source folder identity changed during move"));
                }
                let relative = directory.destination.join(".biblos_uuid");
                let marker = checked_path(&self.destination, &relative)?;
                let original = read_directory_id(&marker)?;
                match original {
                    Some(found) if found == id => {}
                    Some(_) => return Err(failure("destination belongs to another synced folder; its identity was retained")),
                    None => {
                        let mut staged = tempfile::NamedTempFile::new_in(&target)?;
                        staged.write_all(id.to_string().as_bytes())?;
                        staged.as_file().sync_all()?;
                        // Never replace an identity created by another instance.
                        staged.persist_noclobber(&marker).map_err(|error| error.error)?;
                        transfer.markers.push(CreatedMarker { relative, id });
                        sync_directory(&target)?;
                    }
                }
            }
        }
        for file in &mut self.files {
            let source = checked_path(&self.source, &file.source)?;
            let target = checked_path(&self.destination, &file.destination)?;
            validate_snapshot(&source, file)?;
            if target.try_exists()? {
                if file.checksum.is_none() || !matches_snapshot(&target, file)? {
                    return Err(failure(format!("destination file changed: {}", target.display())));
                }
                continue;
            }
            let parent = target.parent().ok_or_else(|| failure("file has no parent"))?;
            let mut staged = tempfile::NamedTempFile::new_in(parent)?;
            let mut reader = HashingReader::new(File::open(&source)?);
            io::copy(&mut reader, &mut staged)?;
            if file.size != Some(reader.size_bytes()) || file.checksum.is_some_and(|checksum| checksum != reader.checksum()) {
                return Err(failure("source file changed during copying; original was retained"));
            }
            validate_snapshot(&source, file)?;
            file.checksum = Some(reader.checksum());
            staged.as_file().set_permissions(fs::metadata(&source)?.permissions())?;
            staged.as_file().sync_all()?;
            checked_path(&self.destination, &file.destination)?;
            staged.persist_noclobber(&target).map_err(|error| error.error)?;
            let mut copied = file.clone();
            copied.source = file.destination.clone();
            transfer.rollback.as_mut().expect("uncommitted transfer").0.files.push(copied);
            sync_directory(parent)?;
        }
        Ok(transfer)
    }

    pub fn validate_commit(&self) -> io::Result<()> {
        let expected = self.files.iter().map(|file| file.source.clone()).chain(self.directories.iter().map(|directory| directory.source.clone())).collect::<HashSet<_>>();
        let mut observed = HashSet::new();
        let mut walk = crate::filesystem::walk(&self.source);
        while let Some(entry) = walk.next() {
            let entry = entry?;
            let relative = entry.path().strip_prefix(&self.source).map_err(|_| failure("source entry is outside the library"))?;
            if managed_identity(relative) {
                if entry.is_dir() {
                    walk.skip_current_dir();
                }
                continue;
            }
            if entry.file_name() == ".biblos_uuid" && relative.parent().is_some_and(|parent| !parent.as_os_str().is_empty()) {
                continue;
            }
            observed.insert(relative.to_owned());
        }
        if observed != expected {
            return Err(failure("source folder contents changed during move; originals were retained"));
        }
        for directory in &self.directories {
            let source_marker = checked_path(&self.source, &directory.source.join(".biblos_uuid"))?;
            if read_directory_id(&source_marker)? != directory.id {
                return Err(failure("source folder identity changed; originals were retained"));
            }
            let target = checked_path(&self.destination, &directory.destination)?;
            if !target.is_dir() {
                return Err(failure("destination folder changed; originals were retained"));
            }
            if let Some(id) = directory.id {
                if read_directory_id(&target.join(".biblos_uuid"))? != Some(id) {
                    return Err(failure("destination folder identity changed; originals were retained"));
                }
            }
        }
        for file in &self.files {
            validate_snapshot(&checked_path(&self.source, &file.source)?, file)?;
            if !matches_snapshot(&checked_path(&self.destination, &file.destination)?, file)? {
                return Err(failure("destination file changed; originals were retained"));
            }
        }
        Ok(())
    }

    /// Remove only unchanged byte snapshots. Committed cleanup never depends
    /// on later changes at the new root. Checkpoints contain only pending files.
    pub fn cleanup_originals(&mut self, check_source: impl Fn() -> io::Result<()>, mut checkpoint: impl FnMut(&Self) -> io::Result<()>) -> io::Result<()> {
        // The durable per-audiobook phase distinguishes a changed revision
        // from our own partial cleanup after interruption.
        check_source()?;
        while !self.files.is_empty() {
            let index = self.files.len() - 1;
            check_source()?;
            if !original_members_present(&self.source, &self.files[index])? {
                self.files.pop();
                checkpoint(self)?;
                continue;
            }
            if self.files[index].folder && !self.files[index].cleanup_started {
                let source = checked_path(&self.source, &self.files[index].source)?;
                if source.try_exists()? && book_checksum(&source)? != self.files[index].checksum()? {
                    return Err(failure("source audiobook changed; original files were retained"));
                }
                self.files[index].cleanup_started = true;
                checkpoint(self)?;
            }
            let book = &self.files[index];
            let source = checked_path(&self.source, &book.source)?;
            if book.folder {
                validate_remaining_members(&source, &book.members)?;
                for member in &book.members {
                    remove_verified(&checked_path(&source, &member.path)?, member.checksum, &check_source)?;
                }
                // Remove empty track directories only; unrelated files keep their folders.
                let mut directories = HashSet::new();
                for member in &book.members {
                    let path = checked_path(&source, &member.path)?;
                    let mut parent = path.parent();
                    while let Some(directory) = parent.filter(|directory| *directory != source) {
                        directories.insert(directory.to_owned());
                        parent = directory.parent();
                    }
                }
                let mut directories = directories.into_iter().collect::<Vec<_>>();
                directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
                for directory in directories {
                    check_source()?;
                    remove_empty(&directory)?;
                }
                check_source()?;
                remove_empty(&source)?;
            } else {
                remove_verified(&source, book.checksum()?, &check_source)?;
            }
            self.files.pop();
            checkpoint(self)?;
        }
        if self.whole_tree {
            while let Some(directory) = self.directories.last() {
                check_source()?;
                let path = checked_path(&self.source, &directory.source)?;
                if path.try_exists()? {
                    if let Some(id) = directory.id {
                        let marker = checked_path(&self.source, &directory.source.join(".biblos_uuid"))?;
                        match read_directory_id(&marker)? {
                            Some(found) if found == id => {
                                check_source()?;
                                fs::remove_file(&marker)?;
                                sync_directory(&path)?;
                            }
                            Some(_) => return Err(failure("source folder identity changed; folder was retained")),
                            None => {}
                        }
                    }
                    check_source()?;
                    remove_empty(&path)?;
                }
                self.directories.pop();
                checkpoint(self)?;
            }
        }
        Ok(())
    }
}

fn read_directory_id(marker: &Path) -> io::Result<Option<DirId>> {
    match fs::symlink_metadata(marker) {
        Ok(metadata) if metadata.is_symlink() || !metadata.is_file() => return Err(failure("folder identity is not an ordinary file")),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let text = fs::read_to_string(marker)?;
    DirId::parse_str(text.trim()).map(Some).map_err(|_| failure("invalid folder identity"))
}

fn validate_remaining_members(source: &Path, expected: &[Member]) -> io::Result<()> {
    let expected = expected.iter().map(|member| (&member.path, member.checksum)).collect::<HashMap<_, _>>();
    for member in members(source, true)? {
        if expected.get(&member.path).copied() != Some(member.checksum) {
            return Err(failure("source audiobook changed during cleanup; remaining original files were retained"));
        }
    }
    Ok(())
}

/// Store identities, manifests and live OS locks are managed by the root commit.
fn managed_identity(relative: &Path) -> bool {
    matches!(relative.to_str(), Some(".bokheim/storage-identity" | ".bokheim/library.json" | ".bokheim-library.json")) || relative.starts_with(".bokheim/assets/leases")
}

fn validate_snapshot(path: &Path, file: &FileMove) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || file.size.is_some_and(|size| size != metadata.len()) || file.modified.is_some_and(|modified| metadata.modified().ok() != Some(modified)) {
        return Err(failure(format!("source file changed during move: {}", path.display())));
    }
    Ok(())
}

fn matches_snapshot(path: &Path, file: &FileMove) -> io::Result<bool> {
    let metadata = fs::symlink_metadata(path)?;
    Ok(metadata.is_file() && file.size.is_none_or(|size| size == metadata.len()) && hash_file(&mut File::open(path)?)? == file.checksum()?)
}

/// Inventory the destination by complete portable paths, not host filesystem
/// equality. Component boundaries remain distinct during Unicode normalization.
struct MergeContext {
    // Some(path) is a unique existing entry. None marks an ambiguous path
    // or a destination already claimed by this plan.
    paths: HashMap<Vec<String>, Option<PathBuf>>,
    destination_ids: HashMap<DirId, PathBuf>,
}
impl MergeContext {
    fn new(destination: &Path) -> io::Result<Self> {
        let mut context = Self { paths: HashMap::new(), destination_ids: HashMap::new() };
        let mut walk = crate::filesystem::walk(destination);
        while let Some(entry) = walk.next() {
            let entry = entry?;
            let relative = entry.path().strip_prefix(destination).map_err(|_| failure("entry is outside destination"))?.to_owned();
            context.paths.entry(Self::key(&relative)).and_modify(|path| *path = None).or_insert_with(|| Some(relative.clone()));
            if managed_identity(&relative) && entry.is_dir() {
                walk.skip_current_dir();
            }
            let hidden = relative.components().any(|component| component.as_os_str().to_string_lossy().starts_with('.'));
            if entry.is_dir() && !hidden {
                if let Some(id) = read_directory_id(&entry.path().join(".biblos_uuid"))? {
                    if id == ROOT_DIR_ID || context.destination_ids.insert(id, relative).is_some() {
                        return Err(failure("destination contains a reserved or duplicate folder identity; originals were retained"));
                    }
                }
            }
        }
        Ok(context)
    }

    fn key(relative: &Path) -> Vec<String> {
        relative.components().map(|component| portable_name_key(&component.as_os_str().to_string_lossy())).collect()
    }

    fn claim(&mut self, relative: &Path) -> bool {
        // Missing and unique paths can be reserved; blocked paths stay blocked.
        !matches!(self.paths.insert(Self::key(relative), None), Some(None))
    }
}

struct IncomingEntry {
    relative: PathBuf,
    name: String,
    directory: bool,
    id: Option<DirId>,
    size: u64,
    modified: Option<SystemTime>,
}

/// Snapshot the complete source tree before resolving any destination paths.
fn incoming_entries(root: &Path) -> io::Result<Vec<IncomingEntry>> {
    let mut entries = Vec::new();
    let mut ids = HashSet::new();
    let mut walk = crate::filesystem::walk(root);
    while let Some(entry) = walk.next() {
        let entry = entry?;
        let relative = entry.path().strip_prefix(root).map_err(|_| failure("entry is outside source"))?.to_owned();
        let parent = relative.parent().ok_or_else(|| failure("source entry has no parent"))?;
        let mut name = entry.file_name().to_str().ok_or_else(|| failure("library paths must contain valid Unicode"))?.to_owned();
        if managed_identity(&relative) {
            if entry.is_dir() {
                walk.skip_current_dir();
            }
            continue;
        }
        if name == ".biblos_uuid" && !parent.as_os_str().is_empty() {
            continue;
        }
        let path = checked_path(root, &relative)?;
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(failure(format!("library move requires an ordinary file or folder: {}", path.display())));
        }
        let id = if metadata.is_dir() { read_directory_id(&path.join(".biblos_uuid"))? } else { None };
        if let Some(id) = id {
            if id == ROOT_DIR_ID || !ids.insert(id) {
                return Err(failure("source contains a reserved or duplicate folder identity; originals were retained"));
            }
        }
        let hidden = relative.components().any(|component| component.as_os_str().to_string_lossy().starts_with('.'));
        if metadata.is_dir() && !hidden {
            name = project_folder_name(&name);
        }
        entries.push(IncomingEntry { relative, name, directory: metadata.is_dir(), id, size: metadata.len(), modified: metadata.modified().ok() });
    }
    // Existing folder IDs decide sibling precedence; unregistered folders use
    // their source paths. The shared name candidates and path index resolve
    // collisions directly, without constructing temporary replica identities.
    entries.sort_by(|left, right| (left.relative.components().count(), !left.directory, Reverse(left.id), &left.relative).cmp(&(right.relative.components().count(), !right.directory, Reverse(right.id), &right.relative)));
    Ok(entries)
}

fn choose_destination(source: &Path, destination: &Path, requested: &Path, entry: &IncomingEntry, reserved: &HashSet<String>, context: &mut MergeContext, checksum: &mut Option<ContentHash>) -> io::Result<PathBuf> {
    let parent = requested.parent().ok_or_else(|| failure("destination has no parent"))?;
    let occupied = reserved.iter().map(String::as_str);
    let alternatives: Box<dyn Iterator<Item = String>> = if entry.directory && !entry.name.starts_with('.') { Box::new(folder_name_candidates(&entry.name, occupied)) } else { Box::new(file_name_candidates(&entry.name, occupied)) };
    for name in std::iter::once(entry.name.clone()).chain(alternatives) {
        let proposed = parent.join(&name);
        let key = MergeContext::key(&proposed);
        let found = match context.paths.get(&key) {
            None => {
                context.claim(&proposed);
                return Ok(proposed);
            }
            Some(Some(found)) => found.clone(),
            Some(None) => continue,
        };
        let target = checked_path(destination, &found)?;
        let metadata = fs::symlink_metadata(&target)?;
        let compatible = if entry.directory && metadata.is_dir() {
            let target_id = read_directory_id(&target.join(".biblos_uuid"))?;
            target_id.is_none() || target_id == entry.id
        } else if !entry.directory && metadata.is_file() && entry.size == metadata.len() {
            let digest = match *checksum {
                Some(digest) => digest,
                None => {
                    let digest = hash_file(&mut File::open(checked_path(source, &entry.relative)?)?)?;
                    *checksum = Some(digest);
                    digest
                }
            };
            hash_file(&mut File::open(&target)?)? == digest
        } else {
            false
        };
        if compatible && context.claim(&found) {
            return Ok(found);
        }
    }
    Err(failure("could not choose a portable destination name"))
}

fn remove_verified(path: &Path, checksum: ContentHash, check_source: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => return Err(failure("old book file was replaced; retained for review")),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    if hash_file(&mut File::open(path)?)? != checksum {
        return Err(failure(format!("old book file changed; retained for review: {}", path.display())));
    }
    check_source()?;
    fs::remove_file(path)?;
    sync_directory(path.parent().ok_or_else(|| failure("book has no parent"))?)
}

fn remove_empty(path: &Path) -> io::Result<()> {
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) => Ok(()),
        Err(error) => Err(error),
    }
}
