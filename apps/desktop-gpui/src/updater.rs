use std::env;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use browser_ui::{DesktopUpdate, DesktopUpdater};
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use ui_runtime::BlockingWorker;

const RELEASE_API: &str = "https://api.github.com/repos/JohanAnderssonOstling/bokheim/releases/latest";
const APPIMAGE_ASSET: &str = "Bokheim-x86_64.AppImage";
const CHECKSUM_ASSET: &str = "Bokheim-x86_64.AppImage.sha256";

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub(crate) fn direct_appimage_updater(worker: BlockingWorker) -> Option<DesktopUpdater> {
    let appimage = eligible_appimage_path_from(env::var_os("BOKHEIM_DISABLE_SELF_UPDATE").is_some(), env::var_os("APPIMAGE"))?;
    let current_version = env!("CARGO_PKG_VERSION").to_owned();

    let check_worker = worker.clone();
    let check = Rc::new(move || check_worker.dispatch(check_latest_release));

    let install_worker = worker;
    let install_path = appimage.clone();
    let install = Rc::new(move |update| {
        let appimage = install_path.clone();
        install_worker.dispatch(move || install_release(&appimage, &update))
    });

    let restart_path = appimage;
    let restart = Rc::new(move || Command::new(&restart_path).spawn().map(|_| ()).map_err(|error| format!("Could not restart the updated AppImage: {error}")));

    Some(DesktopUpdater::new(current_version, check, install, restart))
}

fn eligible_appimage_path_from(disabled: bool, appimage: Option<OsString>) -> Option<PathBuf> {
    if disabled {
        return None;
    }
    let path = PathBuf::from(appimage?);
    if !path.is_file() || OpenOptions::new().write(true).open(&path).is_err() {
        return None;
    }
    Some(path.canonicalize().unwrap_or(path))
}

fn github_client() -> Result<Client, String> {
    Client::builder().user_agent(concat!("Bokheim/", env!("CARGO_PKG_VERSION"))).timeout(Duration::from_secs(60)).build().map_err(|error| format!("Could not initialize the update client: {error}"))
}

fn check_latest_release() -> Result<Option<DesktopUpdate>, String> {
    let release = github_client()?
        .get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Could not check GitHub Releases: {error}"))?
        .json::<GithubRelease>()
        .map_err(|error| format!("GitHub returned invalid release metadata: {error}"))?;

    let current = Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| format!("The installed Bokheim version is invalid: {error}"))?;
    let available = Version::parse(release.tag_name.trim_start_matches('v')).map_err(|error| format!("The latest GitHub release version is invalid: {error}"))?;
    if available <= current {
        return Ok(None);
    }

    let download_url = asset_url(&release.assets, APPIMAGE_ASSET)?;
    let checksum_url = asset_url(&release.assets, CHECKSUM_ASSET)?;
    Ok(Some(DesktopUpdate { version: available.to_string(), release_name: release.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| format!("Bokheim {available}")), download_url, checksum_url }))
}

fn asset_url(assets: &[GithubAsset], name: &str) -> Result<String, String> {
    assets.iter().find(|asset| asset.name == name).map(|asset| asset.browser_download_url.clone()).ok_or_else(|| format!("The latest GitHub release does not contain {name}"))
}

fn install_release(appimage: &Path, update: &DesktopUpdate) -> Result<(), String> {
    let client = github_client()?;
    let expected_checksum = download_checksum(&client, &update.checksum_url)?;
    let file_name = appimage.file_name().and_then(|name| name.to_str()).ok_or_else(|| "The AppImage filename is invalid".to_owned())?;
    let partial = appimage.with_file_name(format!(".{file_name}.{}.download", std::process::id()));
    let backup = appimage.with_file_name(format!("{file_name}.previous"));

    let result = download_appimage(&client, &update.download_url, &partial, &expected_checksum).and_then(|()| replace_appimage(appimage, &partial, &backup));
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

fn download_checksum(client: &Client, url: &str) -> Result<String, String> {
    let text = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Could not download the AppImage checksum: {error}"))?
        .text()
        .map_err(|error| format!("Could not read the AppImage checksum: {error}"))?;
    let checksum = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("The published AppImage checksum is invalid".to_owned());
    }
    Ok(checksum)
}

fn download_appimage(client: &Client, url: &str, partial: &Path, expected_checksum: &str) -> Result<(), String> {
    let mut response = client.get(url).send().and_then(|response| response.error_for_status()).map_err(|error| format!("Could not download the AppImage: {error}"))?;
    let mut output = File::create(partial).map_err(|error| format!("Could not create the AppImage update: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let count = response.read(&mut buffer).map_err(|error| format!("Could not read the AppImage download: {error}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(|error| format!("Could not save the AppImage update: {error}"))?;
    }
    output.sync_all().map_err(|error| format!("Could not finish saving the AppImage update: {error}"))?;
    let actual_checksum = format!("{:x}", digest.finalize());
    if actual_checksum != expected_checksum {
        return Err("The downloaded AppImage failed SHA-256 verification".to_owned());
    }
    let mode = fs::metadata(partial).map_err(|error| error.to_string())?.permissions().mode() | 0o111;
    fs::set_permissions(partial, fs::Permissions::from_mode(mode)).map_err(|error| format!("Could not make the AppImage update executable: {error}"))?;
    Ok(())
}

fn replace_appimage(appimage: &Path, partial: &Path, backup: &Path) -> Result<(), String> {
    if backup.exists() {
        fs::remove_file(backup).map_err(|error| format!("Could not replace the previous AppImage backup: {error}"))?;
    }
    fs::rename(appimage, backup).map_err(|error| format!("Could not back up the current AppImage: {error}"))?;
    if let Err(error) = fs::rename(partial, appimage) {
        let rollback = fs::rename(backup, appimage);
        return Err(match rollback {
            Ok(()) => format!("Could not install the AppImage update; the current version was restored: {error}"),
            Err(rollback_error) => format!("Could not install the AppImage update ({error}) or restore the backup ({rollback_error})"),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_manager_opt_out_always_disables_updates() {
        assert_eq!(eligible_appimage_path_from(true, Some(OsString::from("/tmp/Bokheim.AppImage"))), None);
    }

    #[test]
    fn missing_appimage_environment_disables_updates() {
        assert_eq!(eligible_appimage_path_from(false, None), None);
    }

    #[test]
    fn writable_appimage_enables_updates() {
        let path = env::temp_dir().join(format!("bokheim-updater-{}-writable.AppImage", std::process::id()));
        File::create(&path).unwrap();
        let eligible = eligible_appimage_path_from(false, Some(path.clone().into_os_string()));
        assert_eq!(eligible, Some(path.canonicalize().unwrap()));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn read_only_appimage_disables_updates() {
        let path = env::temp_dir().join(format!("bokheim-updater-{}-readonly.AppImage", std::process::id()));
        File::create(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(eligible_appimage_path_from(false, Some(path.clone().into_os_string())), None);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn asset_lookup_requires_the_exact_release_filename() {
        let assets = vec![GithubAsset { name: APPIMAGE_ASSET.to_owned(), browser_download_url: "https://example.invalid/Bokheim.AppImage".to_owned() }];
        assert!(asset_url(&assets, APPIMAGE_ASSET).is_ok());
        assert!(asset_url(&assets, CHECKSUM_ASSET).is_err());
    }
}
