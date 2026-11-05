use crate::{APP, SPACE_MARGIN, io_error, remove_directory, require_space, write_atomic};
use kobo_package::{Policy, destination, inspect, visit, installed_profile, validate_device};
use semver::Version;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

pub(crate) fn stage(root: &Path, updates: &Path, version: &Version) -> Result<Vec<String>, String> {
    let profile = installed_profile(&fs::read_to_string(root.join(".kobo/version")).map_err(io_error)?).map_err(io_error)?;
    validate_device(File::open(updates.join("package.tar.gz")).map_err(io_error)?, Policy::OnDevice, profile).map_err(io_error)?;
    let package = inspect(File::open(updates.join("package.tar.gz")).map_err(io_error)?, Policy::OnDevice, Some(root)).map_err(io_error)?;
    let mut files = Vec::new();
    let mut backup: u64 = 0;
    let mut growth: u64 = 0;
    let mut largest: u64 = 0;
    for file in &package.files {
        largest = largest.max(file.bytes);
        let existing = destination(root, &file.path, false).map_err(io_error)?;
        let previous_size = if existing.exists() {
            let metadata = fs::metadata(existing).map_err(io_error)?;
            backup = backup.checked_add(metadata.len()).ok_or("Update backup size overflow")?;
            largest = largest.max(metadata.len());
            metadata.len()
        } else {
            0
        };
        growth = growth.checked_add(file.bytes.saturating_sub(previous_size)).ok_or("Update workspace size overflow")?;
        files.push(file.path.to_str().ok_or("Invalid Kobo package path")?.to_owned());
    }
    let staged = updates.join("staged");
    remove_directory(&staged)?;
    // Keep staging and the old program, a complete backup, all growth in the
    // installed files, and one temporary copy of the largest file for either
    // atomic replacement or rollback.
    require_space(updates, package.unpacked_bytes.checked_add(backup).and_then(|n| n.checked_add(growth)).and_then(|n| n.checked_add(largest)).and_then(|n| n.checked_add(SPACE_MARGIN)).ok_or("Update workspace size overflow")?)?;
    fs::create_dir_all(&staged).map_err(io_error)?;
    visit(File::open(updates.join("package.tar.gz")).map_err(io_error)?, Policy::OnDevice, |entry, relative| {
        let target = staged.join(relative);
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(target)?;
        } else {
            fs::create_dir_all(update_files::parent_directory(&target)?)?;
            entry.unpack(&target)?;
        }
        Ok(())
    })
    .map_err(io_error)?;
    let info = fs::read_to_string(staged.join(APP).join("BUILD-INFO.txt")).map_err(io_error)?;
    if !info.lines().any(|line| line == format!("Version: {version}")) {
        return Err("The packaged Kobo version does not match the release. Try again when its Kobo package has been refreshed.".into());
    }
    let mut header = [0_u8; 20];
    File::open(staged.join(APP).join("desktop-gpui-kobo")).and_then(|mut executable| executable.read_exact(&mut header)).map_err(io_error)?;
    if &header[..4] != b"\x7fELF" || header[4] != 1 || header[5] != 1 || header[18..20] != [40, 0] {
        return Err("The release does not contain a 32-bit ARM Kobo application".into());
    }
    Ok(files)
}

fn copy_atomic(source: &Path, target: &Path) -> Result<(), String> {
    let temporary = target.with_file_name(format!("{}.bokheim-update-tmp", target.file_name().ok_or("Invalid update filename")?.to_string_lossy()));
    update_files::copy_atomic(source, target, &temporary).map_err(io_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use kobo_package::REQUIRED_FILES;

    fn archive(info: &str) -> Vec<u8> {
        let mut archive = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::fast()));
        let mut elf = [0_u8; 24];
        elf[..6].copy_from_slice(b"\x7fELF\x01\x01");
        elf[18] = 40;
        for path in REQUIRED_FILES.iter().copied().chain([".adds/bokheim/update-recovery.sh", ".adds/bokheim/BUILD-INFO.txt"]) {
            let bytes = if path.ends_with("BUILD-INFO.txt") { info.as_bytes() } else { &elf };
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            archive.append_data(&mut header, path, bytes).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn on_device_staging_checks_the_model_before_creating_program_files() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".kobo")).unwrap();
        fs::write(root.path().join(".kobo/version"), "serial,kernel,4.38.1,x,x,376").unwrap();
        let updates = root.path().join(APP).join(".update");
        fs::create_dir_all(&updates).unwrap();
        fs::write(updates.join("package.tar.gz"), archive("Version: 2.0.0\nCPU profile: libra2\n")).unwrap();
        assert!(stage(root.path(), &updates, &Version::new(2, 0, 0)).is_err());
        assert!(!updates.join("staged").exists());
        fs::write(updates.join("package.tar.gz"), archive("Version: 2.0.0\nCPU profile: generic\nSupported model IDs: 376,388\n")).unwrap();
        let files = stage(root.path(), &updates, &Version::new(2, 0, 0)).unwrap();
        assert!(files.iter().any(|file| file.ends_with("desktop-gpui-kobo")));
        assert!(updates.join("staged/.adds/bokheim/desktop-gpui-kobo").is_file());
        assert!(!root.path().join(APP).join("desktop-gpui-kobo").exists());
    }
}

pub(crate) fn install(root: &Path, updates: &Path, files: &[String]) -> Result<(), String> {
    let backup = updates.join("backup");
    remove_directory(&backup)?;
    fs::create_dir_all(&backup).map_err(io_error)?;
    let mut previous = Vec::new();
    for relative in files {
        let target = destination(root, Path::new(relative), false).map_err(io_error)?;
        if target.exists() {
            copy_atomic(&target, &backup.join(relative))?;
            previous.push(relative.clone());
        }
    }
    write_atomic(&updates.join("files"), format!("{}\n", files.join("\n")).as_bytes())?;
    write_atomic(&updates.join("previous"), format!("{}\n", previous.join("\n")).as_bytes())?;
    write_atomic(&updates.join("installing"), b"1\n")?;
    update_files::sync_dir(updates).map_err(io_error)?;
    // From this point, the firmware shell restores the complete backup on the
    // next launch if any write, process, or power loss interrupts installation.
    for relative in files {
        copy_atomic(&updates.join("staged").join(relative), &destination(root, Path::new(relative), false).map_err(io_error)?)?;
    }
    crate::remove_file(&updates.join("installing"))?;
    update_files::sync_dir(updates).map_err(io_error)?;
    Ok(())
}
