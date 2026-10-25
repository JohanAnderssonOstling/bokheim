//! Download Kobo releases while the UI runs; replace files only after it exits.
//! The firmware shell can undo an interrupted installation without loading any
//! of the application runtime being replaced.

mod package;
mod artifact;

use fs2::FileExt;
pub use kobo_package::Release;
use kobo_package::{APP, RELEASE_API, installed_profile, newer_release_for_model};
use reqwest::blocking::Client;
use semver::Version;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use artifact::{stage_verified, verify_file};

const SPACE_MARGIN: u64 = 16 * 1024 * 1024;

static INSTALLATION: OnceLock<Installation> = OnceLock::new();

#[derive(Clone)]
pub struct Installation {
    root: PathBuf,
    pub version: Version,
}

/// Set by the Kobo executable, whose version is the application release version.
pub fn initialize(root: PathBuf, version: &str) -> Result<(), String> {
    let installation = Installation { root, version: Version::parse(version).map_err(|error| error.to_string())? };
    INSTALLATION.set(installation).map_err(|_| "Kobo updater was already initialized".to_owned())
}

pub fn installation() -> Result<&'static Installation, String> {
    INSTALLATION.get().ok_or_else(|| "Kobo updater is unavailable in this launcher".to_owned())
}

fn client() -> Result<Client, String> {
    Client::builder().https_only(true).connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(15 * 60)).user_agent("Bokheim-Kobo-Updater").build().map_err(|error| error.to_string())
}

impl Installation {
    fn updates(&self) -> PathBuf {
        self.root.join(APP).join(".update")
    }

    fn lock(&self) -> Result<File, String> {
        if !self.root.join(".kobo").is_dir() || !self.root.join(APP).join("desktop-gpui-kobo").is_file() {
            return Err("The Kobo installation could not be found. Restart Bokheim and try again.".into());
        }
        fs::create_dir_all(self.updates()).map_err(io_error)?;
        let lock = OpenOptions::new().create(true).truncate(false).write(true).open(self.updates().join("lock")).map_err(io_error)?;
        lock.try_lock_exclusive().map_err(|_| "Another Kobo update is already running".to_owned())?;
        Ok(lock)
    }

    pub fn pending(&self) -> Option<Release> {
        let release: Release = serde_json::from_slice(&fs::read(self.updates().join("ready.json")).ok()?).ok()?;
        (release.version > self.version).then_some(release)
    }

    pub fn last_error(&self) -> Option<String> {
        fs::read_to_string(self.updates().join("error")).ok()
    }

    pub fn check(&self) -> Result<Option<Release>, String> {
        let response = client()?.get(RELEASE_API).timeout(Duration::from_secs(30)).header("Accept", "application/vnd.github+json").send().and_then(|response| response.error_for_status()).map_err(network_error)?;
        let profile = installed_profile(&fs::read_to_string(self.root.join(".kobo/version")).map_err(io_error)?).map_err(io_error)?;
        newer_release_for_model(response, &self.version, profile.id)
    }

    pub fn download(&self, release: &Release, progress: impl FnMut(u64, u64)) -> Result<(), String> {
        let _lock = self.lock()?;
        release.validate()?;
        if release.version <= self.version {
            return Err("This Kobo release is not newer than the installed version".into());
        }
        let updates = self.updates();
        if updates.join("installing").exists() {
            return Err("Restart Bokheim to recover the interrupted update first".into());
        }
        remove_file(&updates.join("ready.json"))?;
        remove_directory(&updates.join("staged"))?;
        remove_directory(&updates.join("backup"))?;
        remove_file(&updates.join("package.tar.gz"))?;
        remove_file(&updates.join("error"))?;
        let partial = updates.join("download.part");
        remove_file(&partial)?;
        require_space(&updates, release.bytes + SPACE_MARGIN)?;
        let result = (|| {
            let input = client()?.get(&release.url).send().and_then(|response| response.error_for_status()).map_err(network_error)?;
            stage_verified(input, &updates.join("package.tar.gz"), &partial, &release.artifact(), progress)?;
            package::stage(&self.root, &updates, &release.version)?;
            write_atomic(&updates.join("ready.json"), &serde_json::to_vec(release).map_err(|error| error.to_string())?)
        })();
        if result.is_err() {
            let _ = fs::remove_file(partial);
        }
        result
    }

    /// Called by the launcher after the application has shut down and flushed
    /// its databases. No user database or preference file belongs to a package.
    pub fn apply(&self) -> Result<(), String> {
        let _lock = self.lock()?;
        let release = self.pending().ok_or("There is no newer verified Kobo update ready to install")?;
        release.validate()?;
        let updates = self.updates();
        verify_file(&updates.join("package.tar.gz"), &release.artifact())?;
        // Recreate staging from the checked archive rather than trusting files
        // left behind by an earlier interrupted preparation.
        let files = package::stage(&self.root, &updates, &release.version)?;
        package::install(&self.root, &updates, &files)?;
        // A committed installation stays successful even if cleanup fails.
        let _ = remove_file(&updates.join("ready.json"));
        let _ = remove_file(&updates.join("package.tar.gz"));
        let _ = remove_directory(&updates.join("staged"));
        Ok(())
    }
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    update_files::write_atomic(path, &path.with_extension("tmp"), contents, None, |_| {}).map(|_| ()).map_err(io_error)
}

fn require_space(path: &Path, bytes: u64) -> Result<(), String> {
    let free = fs2::available_space(path).map_err(io_error)?;
    if free < bytes {
        return Err(format!("Free up {} MB on your Kobo, then retry the update.", (bytes - free).div_ceil(1_000_000)));
    }
    Ok(())
}

fn remove_file(path: &Path) -> Result<(), String> {
    update_files::remove_file_if_exists(path).map_err(io_error)
}
fn remove_directory(path: &Path) -> Result<(), String> {
    update_files::remove_dir_if_exists(path).map_err(io_error)
}

fn io_error(error: std::io::Error) -> String {
    format!("Could not prepare the Kobo update: {error}")
}
fn network_error(error: reqwest::Error) -> String {
    eprintln!("Kobo update request failed: {error}");
    "Could not download the update. Check your Wi-Fi connection and try again.".into()
}
