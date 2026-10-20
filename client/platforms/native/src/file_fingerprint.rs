//! Native file observation shared by ingestion and local reader opening.
#[cfg(not(windows))]
use std::fs;
#[cfg(windows)]
use std::fs::File;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{io, path::Path, time::UNIX_EPOCH};

pub fn file_fingerprint(path: impl AsRef<Path>) -> io::Result<u64> {
    #[cfg(windows)]
    let file = File::open(path)?;
    #[cfg(windows)]
    let metadata = file.metadata()?;
    #[cfg(not(windows))]
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?.duration_since(UNIX_EPOCH).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&modified.as_nanos().to_le_bytes());
    hasher.update(&metadata.len().to_le_bytes());
    #[cfg(unix)]
    {
        hasher.update(&metadata.ino().to_le_bytes());
        hasher.update(&metadata.dev().to_le_bytes());
        // A copier can replace bytes in place and restore the original mtime.
        // Unix ctime still changes, so the scanner must inspect that file again.
        hasher.update(&metadata.ctime().to_le_bytes());
        hasher.update(&metadata.ctime_nsec().to_le_bytes());
    }
    #[cfg(windows)]
    hash_windows_revision(&file, &mut hasher)?;
    Ok(u64::from_le_bytes(hasher.finalize().as_bytes()[..8].try_into().expect("BLAKE3 digest contains eight bytes")))
}

#[cfg(windows)]
fn hash_windows_revision(file: &File, hasher: &mut blake3::Hasher) -> io::Result<()> {
    use std::mem::{size_of, MaybeUninit};
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{FileBasicInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO};

    let mut basic = MaybeUninit::<FILE_BASIC_INFO>::uninit();
    // SAFETY: the live file owns the handle; the writable buffer has the size
    // and alignment required by FileBasicInfo. Success initializes the buffer.
    if unsafe { GetFileInformationByHandleEx(file.as_raw_handle(), FileBasicInfo, basic.as_mut_ptr().cast(), size_of::<FILE_BASIC_INFO>() as u32) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetFileInformationByHandleEx succeeded above.
    let basic = unsafe { basic.assume_init() };
    let mut identity = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the same live handle and a correctly sized, aligned output buffer.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), identity.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetFileInformationByHandle succeeded above.
    let identity = unsafe { identity.assume_init() };
    // ChangeTime catches in-place edits with a restored LastWriteTime; file
    // identity catches replacement files with copied timestamps. Access time
    // is deliberately excluded so reading a book never invalidates its revision.
    hasher.update(&basic.ChangeTime.to_le_bytes());
    hasher.update(&basic.CreationTime.to_le_bytes());
    hasher.update(&identity.dwVolumeSerialNumber.to_le_bytes());
    hasher.update(&identity.nFileIndexHigh.to_le_bytes());
    hasher.update(&identity.nFileIndexLow.to_le_bytes());
    Ok(())
}

#[cfg(all(test, any(unix, windows)))]
mod tests {
    use super::*;
    use std::fs::{self, File};

    #[test]
    fn detects_in_place_replacement_with_restored_mtime() {
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), b"first").unwrap();
        let before = fs::metadata(file.path()).unwrap();
        let fingerprint = file_fingerprint(file.path()).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(file.path(), b"other").unwrap();
        File::options().write(true).open(file.path()).unwrap().set_modified(before.modified().unwrap()).unwrap();
        let after = fs::metadata(file.path()).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        #[cfg(unix)]
        assert_eq!(before.ino(), after.ino());
        assert_ne!(fingerprint, file_fingerprint(file.path()).unwrap());
    }

    #[test]
    fn detects_replacement_with_matching_size_and_mtime() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.epub");
        fs::write(&path, b"first").unwrap();
        // Keep the old identity alive while the replacement is created.
        let original = File::open(&path).unwrap();
        let before = original.metadata().unwrap();
        let fingerprint = file_fingerprint(&path).unwrap();
        let replacement = directory.path().join("replacement.epub");
        fs::write(&replacement, b"other").unwrap();
        File::options().write(true).open(&replacement).unwrap().set_modified(before.modified().unwrap()).unwrap();
        // Windows cannot replace the old destination while this handle is
        // open. The replacement already exists with its own distinct identity.
        drop(original);
        fs::rename(&replacement, &path).unwrap();
        let after = fs::metadata(&path).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(before.modified().unwrap(), after.modified().unwrap());
        assert_ne!(fingerprint, file_fingerprint(&path).unwrap());
    }

    #[test]
    fn reading_an_unchanged_file_keeps_its_fingerprint() {
        let file = tempfile::NamedTempFile::new().unwrap();
        fs::write(file.path(), b"book").unwrap();
        let fingerprint = file_fingerprint(file.path()).unwrap();
        assert_eq!(fs::read(file.path()).unwrap(), b"book");
        assert_eq!(fingerprint, file_fingerprint(file.path()).unwrap());
    }
}
