use std::env;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use browser_ui::{DesktopUpdate, DesktopUpdater};
use reqwest::blocking::Client;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use ui_runtime::BlockingWorker;

const RELEASE_API: &str = "https://api.github.com/repos/JohanAnderssonOstling/bokheim/releases/latest";
const INSTALLER_ASSET: &str = "Bokheim-Windows-x86_64-Setup.exe";
const CHECKSUM_ASSET: &str = "Bokheim-Windows-x86_64-Setup.exe.sha256";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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

#[derive(Clone)]
struct StagedUpdate {
    installer: PathBuf,
    helper: PathBuf,
}

pub(crate) fn windows_installer_updater(worker: BlockingWorker) -> Option<DesktopUpdater> {
    if env::var_os("BOKHEIM_DISABLE_SELF_UPDATE").is_some() {
        return None;
    }

    let executable = env::current_exe().ok()?;
    let executable = executable.canonicalize().unwrap_or(executable);
    let staged = Arc::new(Mutex::new(None::<StagedUpdate>));

    let check_worker = worker.clone();
    let check = Rc::new(move || check_worker.dispatch(check_latest_release));

    let install_worker = worker;
    let install_state = Arc::clone(&staged);
    let install = Rc::new(move |update| {
        let state = Arc::clone(&install_state);
        install_worker.dispatch(move || {
            let staged_update = download_release(&update)?;
            *state.lock().map_err(|_| "The Windows updater state is unavailable".to_owned())? = Some(staged_update);
            Ok(())
        })
    });

    let restart_state = staged;
    let restart_executable = executable;
    let restart = Rc::new(move || {
        let staged_update = restart_state.lock().map_err(|_| "The Windows updater state is unavailable".to_owned())?.clone().ok_or_else(|| "No verified Windows update is ready to install".to_owned())?;
        launch_update_helper(&restart_executable, &staged_update)
    });

    Some(DesktopUpdater::new(env!("CARGO_PKG_VERSION"), check, install, restart))
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

    Ok(Some(DesktopUpdate {
        version: available.to_string(),
        release_name: release.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| format!("Bokheim {available}")),
        download_url: asset_url(&release.assets, INSTALLER_ASSET)?,
        checksum_url: asset_url(&release.assets, CHECKSUM_ASSET)?,
    }))
}

fn asset_url(assets: &[GithubAsset], name: &str) -> Result<String, String> {
    assets.iter().find(|asset| asset.name == name).map(|asset| asset.browser_download_url.clone()).ok_or_else(|| format!("The latest GitHub release does not contain {name}"))
}

fn download_release(update: &DesktopUpdate) -> Result<StagedUpdate, String> {
    let client = github_client()?;
    let expected_checksum = download_checksum(&client, &update.checksum_url)?;
    let directory = env::temp_dir().join("bokheim-updater").join(format!("{}-{}", update.version, std::process::id()));
    fs::create_dir_all(&directory).map_err(|error| format!("Could not create the Windows update directory: {error}"))?;
    let partial = directory.join("Bokheim-Setup.exe.download");
    let installer = directory.join("Bokheim-Setup.exe");
    let helper = directory.join("apply-update.ps1");
    let _ = fs::remove_file(&partial);
    let _ = fs::remove_file(&installer);

    let result = download_installer(&client, &update.download_url, &partial, &expected_checksum).and_then(|()| fs::rename(&partial, &installer).map_err(|error| format!("Could not stage the verified Windows installer: {error}")));
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }

    write_update_helper(&helper)?;
    Ok(StagedUpdate { installer, helper })
}

fn download_checksum(client: &Client, url: &str) -> Result<String, String> {
    let text = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("Could not download the Windows checksum: {error}"))?
        .text()
        .map_err(|error| format!("Could not read the Windows checksum: {error}"))?;
    let checksum = text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("The published Windows checksum is invalid".to_owned());
    }
    Ok(checksum)
}

fn download_installer(client: &Client, url: &str, partial: &Path, expected_checksum: &str) -> Result<(), String> {
    let mut response = client.get(url).send().and_then(|response| response.error_for_status()).map_err(|error| format!("Could not download the Windows update: {error}"))?;
    let mut output = File::create(partial).map_err(|error| format!("Could not create the Windows update: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let count = response.read(&mut buffer).map_err(|error| format!("Could not read the Windows download: {error}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(|error| format!("Could not save the Windows update: {error}"))?;
    }
    output.sync_all().map_err(|error| format!("Could not finish saving the Windows update: {error}"))?;
    if format!("{:x}", digest.finalize()) != expected_checksum {
        return Err("The downloaded Windows installer failed SHA-256 verification".to_owned());
    }

    let mut signature = [0_u8; 2];
    File::open(partial).and_then(|mut file| file.read_exact(&mut signature)).map_err(|error| format!("Could not validate the Windows installer: {error}"))?;
    if signature != *b"MZ" {
        return Err("The downloaded update is not a Windows installer".to_owned());
    }
    Ok(())
}

fn write_update_helper(path: &Path) -> Result<(), String> {
    let script = r#"param(
    [Parameter(Mandatory=$true)][uint32]$ProcessId,
    [Parameter(Mandatory=$true)][string]$Target,
    [Parameter(Mandatory=$true)][string]$Installer
)
$ErrorActionPreference = 'Stop'
$Log = Join-Path $env:TEMP 'bokheim-updater.log'
try {
    Wait-Process -Id $ProcessId -ErrorAction SilentlyContinue
    $Setup = Start-Process -FilePath $Installer -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-') -Wait -PassThru
    if ($Setup.ExitCode -ne 0) {
        throw "Bokheim installer exited with code $($Setup.ExitCode)"
    }
    Start-Process -FilePath $Target
    Remove-Item -LiteralPath $Installer -Force
    Remove-Item -LiteralPath $PSCommandPath -Force
} catch {
    $_ | Out-File -FilePath $Log -Encoding utf8 -Append
}
"#;
    fs::write(path, script).map_err(|error| format!("Could not create the Windows update helper: {error}"))
}

fn launch_update_helper(current: &Path, staged: &StagedUpdate) -> Result<(), String> {
    Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&staged.helper)
        .arg("-ProcessId")
        .arg(std::process::id().to_string())
        .arg("-Target")
        .arg(current)
        .arg("-Installer")
        .arg(&staged.installer)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not start the Windows update helper: {error}"))
}
