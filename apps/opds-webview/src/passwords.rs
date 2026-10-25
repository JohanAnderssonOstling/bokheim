//! Device-local browser passwords. The profile holds only site/account labels;
//! secrets live in the OS credential store, outside all sync contracts.
use keyring::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use url::Url;
use zeroize::Zeroizing;

const SERVICE: &str = "se.bokheim.browser.passwords";
const STORAGE_ERROR: &str = "Could not access this device's password store. Unlock your system keyring and try again.";
const INDEX_ERROR: &str = "Could not read or save the local password list.";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct SavedLogin {
    pub origin: String,
    pub username: String,
}

// Never derive Debug for values containing passwords.
pub struct Login {
    pub site: SavedLogin,
    pub password: Zeroizing<String>,
}

pub enum Command {
    Lookup {
        origin: String,
        username: String,
    },
    #[cfg(test)]
    List,
    Save(Login),
    #[cfg(test)]
    Delete(SavedLogin),
}
pub enum Reply {
    Match(Option<Login>),
    #[cfg(test)]
    Listed(Vec<SavedLogin>),
    Saved,
    #[cfg(test)]
    Deleted,
}

/// Exact scheme/host/port matching. Plain HTTP is allowed only on loopback
/// addresses, so local development does not need a TLS certificate.
pub fn origin(address: &str) -> Option<String> {
    let url = Url::parse(address).ok()?;
    let loopback = url.host_str().is_some_and(|host| host == "localhost" || host == "[::1]" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()));
    (url.host_str().is_some() && url.username().is_empty() && url.password().is_none() && (url.scheme() == "https" || (url.scheme() == "http" && loopback))).then(|| url.origin().ascii_serialization())
}

pub fn fill_script(login: &Login, nonce: &str) -> Result<Zeroizing<String>, serde_json::Error> {
    let password = Zeroizing::new(serde_json::to_string(login.password.as_str())?);
    let args = Zeroizing::new(format!(
        "{{\"kind\":\"fill\",\"nonce\":{},\"origin\":{},\"username\":{},\"password\":{}}}",
        serde_json::to_string(nonce)?,
        serde_json::to_string(&login.site.origin)?,
        serde_json::to_string(&login.site.username)?,
        password.as_str()
    ));
    Ok(Zeroizing::new(format!("({})({})", include_str!("passwords.js"), args.as_str())))
}

pub(crate) trait SecretStore {
    fn read(&self, key: &str) -> Result<Zeroizing<String>, String>;
    fn write(&self, key: &str, password: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}
pub struct SystemSecrets;

fn entry(key: &str) -> Result<Entry, String> {
    #[cfg(target_os = "windows")]
    let result = Entry::new_with_target(key, SERVICE, key);
    #[cfg(not(target_os = "windows"))]
    let result = Entry::new(SERVICE, key);
    result.map_err(|_| STORAGE_ERROR.to_owned())
}

impl SecretStore for SystemSecrets {
    fn read(&self, key: &str) -> Result<Zeroizing<String>, String> {
        let bytes = Zeroizing::new(entry(key)?.get_secret().map_err(|_| STORAGE_ERROR.to_owned())?);
        let password = std::str::from_utf8(&bytes).map_err(|_| STORAGE_ERROR.to_owned())?;
        Ok(Zeroizing::new(password.to_owned()))
    }
    fn write(&self, key: &str, password: &str) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        {
            use windows::Win32::Security::Credentials::{CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW};
            use windows::core::PWSTR;
            let mut target: Vec<u16> = key.encode_utf16().chain(Some(0)).collect();
            let mut username: Vec<u16> = SERVICE.encode_utf16().chain(Some(0)).collect();
            let mut secret = Zeroizing::new(password.as_bytes().to_vec());
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target.as_mut_ptr()),
                UserName: PWSTR(username.as_mut_ptr()),
                CredentialBlob: secret.as_mut_ptr(),
                CredentialBlobSize: secret.len() as u32,
                // Keyring defaults to ENTERPRISE, which can roam. Explicitly
                // keep these credentials on this machine instead.
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                ..Default::default()
            };
            // All pointers remain owned and valid during this synchronous call.
            unsafe { CredWriteW(&credential, 0) }.map_err(|_| STORAGE_ERROR.to_owned())
        }
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            entry(key)?.set_secret(password.as_bytes()).map_err(|_| STORAGE_ERROR.to_owned())
        }
        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            let _ = (key, password);
            Err("Password storage is unavailable on this platform.".into())
        }
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        match entry(key)?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(_) => Err(STORAGE_ERROR.into()),
        }
    }
}

pub struct Vault<S = SystemSecrets> {
    directory: PathBuf,
    secrets: S,
}
impl Vault {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory, secrets: SystemSecrets }
    }
}

impl<S: SecretStore> Vault<S> {
    fn key(&self, site: &SavedLogin) -> String {
        let mut hash = blake3::Hasher::new();
        for part in [self.directory.to_string_lossy().as_bytes(), site.origin.as_bytes(), site.username.as_bytes()] {
            hash.update(&(part.len() as u64).to_le_bytes());
            hash.update(part);
        }
        format!("{SERVICE}.{}", hash.finalize().to_hex())
    }
    fn list(&self) -> Result<Vec<SavedLogin>, String> {
        match fs::read(self.directory.join("passwords.json")) {
            Ok(bytes) => {
                let mut sites: Vec<SavedLogin> = serde_json::from_slice(&bytes).map_err(|_| INDEX_ERROR.to_owned())?;
                if sites.iter().any(|site| origin(&site.origin).as_deref() != Some(site.origin.as_str()) || site.username.len() > 4096) {
                    return Err(INDEX_ERROR.into());
                }
                sites.sort();
                sites.dedup();
                Ok(sites)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(_) => Err(INDEX_ERROR.into()),
        }
    }
    fn persist(&self, sites: &[SavedLogin]) -> Result<(), String> {
        (|| -> io::Result<()> {
            // tempfile creates files with 0600 on Unix; replacement is atomic.
            // Only site/account labels are serialized here, never passwords.
            let mut file = NamedTempFile::new_in(&self.directory)?;
            file.write_all(&serde_json::to_vec(sites)?)?;
            file.as_file().sync_all()?;
            file.persist(self.directory.join("passwords.json")).map_err(|error| error.error)?;
            Ok(())
        })()
        .map_err(|_| INDEX_ERROR.to_owned())
    }
    fn lock(&self) -> Result<File, String> {
        let file = File::options().read(true).write(true).create(true).truncate(false).open(self.directory.join("passwords.lock")).map_err(|_| INDEX_ERROR.to_owned())?;
        file.lock().map_err(|_| INDEX_ERROR.to_owned())?;
        Ok(file)
    }
    /// Run off the UI thread. File locking merges saves/deletes from every
    /// browser process before updating the index.
    pub fn execute(&self, command: Command) -> Result<Reply, String> {
        let _lock = self.lock()?;
        let mut sites = self.list()?;
        match command {
            Command::Lookup { origin: requested, username } => {
                if origin(&requested).as_deref() != Some(requested.as_str()) || username.len() > 4096 {
                    return Ok(Reply::Match(None));
                }
                let mut matching = sites.into_iter().filter(|site| site.origin == requested && (username.is_empty() || site.username == username));
                let Some(site) = matching.next() else {
                    return Ok(Reply::Match(None));
                };
                // With several accounts, wait for a matching username instead
                // of silently choosing an account for the user.
                if matching.next().is_some() {
                    return Ok(Reply::Match(None));
                }
                let password = self.secrets.read(&self.key(&site))?;
                Ok(Reply::Match(Some(Login { site, password })))
            }
            #[cfg(test)]
            Command::List => Ok(Reply::Listed(sites)),
            Command::Save(login) => {
                if origin(&login.site.origin).as_deref() != Some(login.site.origin.as_str()) || login.site.username.len() > 4096 || login.password.is_empty() || login.password.len() > 2560 {
                    return Err("Enter a password of at most 2560 bytes for a secure website.".into());
                }
                let key = self.key(&login.site);
                self.secrets.write(&key, &login.password)?;
                if !sites.contains(&login.site) {
                    sites.push(login.site);
                    sites.sort();
                    if let Err(error) = self.persist(&sites) {
                        let _ = self.secrets.delete(&key);
                        return Err(error);
                    }
                }
                Ok(Reply::Saved)
            }
            #[cfg(test)]
            Command::Delete(site) => {
                if sites.contains(&site) {
                    self.secrets.delete(&self.key(&site))?;
                    sites.retain(|saved| saved != &site);
                    self.persist(&sites)?;
                }
                Ok(Reply::Deleted)
            }
        }
    }
}

pub fn profile_directory(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemorySecrets(Arc<Mutex<HashMap<String, Zeroizing<String>>>>);
    impl SecretStore for MemorySecrets {
        fn read(&self, key: &str) -> Result<Zeroizing<String>, String> {
            self.0.lock().unwrap().get(key).cloned().ok_or_else(|| STORAGE_ERROR.into())
        }
        fn write(&self, key: &str, password: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(key.into(), Zeroizing::new(password.into()));
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn login(origin: &str, username: &str, password: &str) -> Login {
        Login { site: SavedLogin { origin: origin.into(), username: username.into() }, password: Zeroizing::new(password.into()) }
    }

    #[test]
    fn autofill_script_preserves_special_characters_in_credentials() {
        let login = login("https://example.com", "reader\"\\\n", "secret\"\\\n)(\u{0}日本語");
        let nonce = "document\"\\\n";
        let script = fill_script(&login, nonce).unwrap();
        let prefix = format!("({})(", include_str!("passwords.js"));
        let arguments = script.strip_prefix(&prefix).unwrap().strip_suffix(')').unwrap();
        let value: serde_json::Value = serde_json::from_str(arguments).unwrap();
        assert_eq!(value["kind"], "fill");
        assert_eq!(value["origin"], login.site.origin);
        assert_eq!(value["username"], login.site.username);
        assert_eq!(value["password"], login.password.as_str());
        assert_eq!(value["nonce"], nonce);
    }

    #[test]
    fn passwords_stay_out_of_profile_files_and_survive_reopening() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        let vault = Vault { directory: directory.path().into(), secrets: secrets.clone() };
        let site = login("https://example.com", "reader", "first-secret").site;
        vault.execute(Command::Save(login(&site.origin, &site.username, "first-secret"))).unwrap();
        vault.execute(Command::Save(login(&site.origin, &site.username, "updated-secret"))).unwrap();
        let reopened = Vault { directory: directory.path().into(), secrets: secrets.clone() };
        let Reply::Listed(sites) = reopened.execute(Command::List).unwrap() else { panic!() };
        assert_eq!(sites, [site.clone()]);
        let Reply::Match(Some(saved)) = reopened.execute(Command::Lookup { origin: site.origin.clone(), username: site.username.clone() }).unwrap() else { panic!() };
        assert_eq!(saved.password.as_str(), "updated-secret");
        for file in fs::read_dir(directory.path()).unwrap() {
            let bytes = fs::read(file.unwrap().path()).unwrap();
            assert!(!bytes.windows(6).any(|bytes| bytes == b"secret"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(directory.path().join("passwords.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        reopened.execute(Command::Delete(site.clone())).unwrap();
        assert!(secrets.0.lock().unwrap().is_empty());
        assert!(matches!(reopened.execute(Command::Lookup { origin: site.origin, username: site.username }).unwrap(), Reply::Match(None)));
    }
    #[test]
    fn concurrent_windows_preserve_each_others_logins() {
        let directory = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        std::thread::scope(|scope| {
            for index in 0..8 {
                let vault = Vault { directory: directory.path().into(), secrets: secrets.clone() };
                scope.spawn(move || vault.execute(Command::Save(login("https://example.com", &format!("reader-{index}"), "secret"))).map(|_| ()).unwrap());
            }
        });
        let vault = Vault { directory: directory.path().into(), secrets };
        let Reply::Listed(sites) = vault.execute(Command::List).unwrap() else { panic!() };
        assert_eq!(sites.len(), 8);
        for site in sites {
            assert!(matches!(vault.execute(Command::Lookup { origin: site.origin, username: site.username }).unwrap(), Reply::Match(Some(_))));
        }
    }

    #[test]
    fn autofill_waits_for_a_username_when_several_accounts_match() {
        let directory = tempfile::tempdir().unwrap();
        let vault = Vault { directory: directory.path().into(), secrets: MemorySecrets::default() };
        vault.execute(Command::Save(login("https://example.com", "alice", "alice-secret"))).unwrap();
        let Reply::Match(Some(saved)) = vault.execute(Command::Lookup { origin: "https://example.com".into(), username: String::new() }).unwrap() else { panic!() };
        assert_eq!(saved.site.username, "alice");
        vault.execute(Command::Save(login("https://example.com", "bob", "bob-secret"))).unwrap();
        for (origin, username) in [("https://example.com", ""), ("https://example.com", "unknown"), ("https://other.test", "bob"), ("http://example.com", "bob")] {
            assert!(matches!(vault.execute(Command::Lookup { origin: origin.into(), username: username.into() }).unwrap(), Reply::Match(None)));
        }
        let Reply::Match(Some(saved)) = vault.execute(Command::Lookup { origin: "https://example.com".into(), username: "bob".into() }).unwrap() else { panic!() };
        assert_eq!(saved.password.as_str(), "bob-secret");
    }
    #[test]
    fn profile_site_port_and_account_have_separate_secret_namespaces() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        let vault = Vault { directory: first.path().into(), secrets: secrets.clone() };
        let other = Vault { directory: second.path().into(), secrets };
        let site = login("https://example.com", "reader", "secret").site;
        let keys = [
            vault.key(&site),
            other.key(&site),
            vault.key(&SavedLogin { origin: "https://example.com:8443".into(), ..site.clone() }),
            vault.key(&SavedLogin { username: "other".into(), ..site.clone() }),
            vault.key(&SavedLogin { origin: "https://sub.example.com".into(), ..site }),
        ];
        assert_eq!(keys.len(), keys.iter().collect::<HashSet<_>>().len());
    }
    #[test]
    fn unavailable_keyring_and_corrupt_index_never_fall_back_to_plaintext() {
        struct Locked;
        impl SecretStore for Locked {
            fn read(&self, _: &str) -> Result<Zeroizing<String>, String> {
                Err(STORAGE_ERROR.into())
            }
            fn write(&self, _: &str, _: &str) -> Result<(), String> {
                Err(STORAGE_ERROR.into())
            }
            fn delete(&self, _: &str) -> Result<(), String> {
                Err(STORAGE_ERROR.into())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let vault = Vault { directory: directory.path().into(), secrets: Locked };
        assert!(vault.execute(Command::Save(login("https://example.com", "reader", "never-on-disk"))).is_err());
        assert!(!directory.path().join("passwords.json").exists());
        fs::write(directory.path().join("passwords.json"), "corrupt").unwrap();
        let vault = Vault { directory: directory.path().into(), secrets: MemorySecrets::default() };
        assert!(vault.execute(Command::Save(login("https://example.com", "reader", "never-on-disk"))).is_err());
        assert!(vault.secrets.0.lock().unwrap().is_empty());
        assert_eq!(fs::read_to_string(directory.path().join("passwords.json")).unwrap(), "corrupt");
    }
    #[test]
    fn only_secure_canonical_origins_are_stored() {
        for url in ["http://example.com", "file:///etc/passwd", "about:blank", "https://user:pass@example.com", "data:text/html,test"] {
            assert_eq!(origin(url), None);
        }
        assert_eq!(origin("https://EXAMPLE.com:443/login?a=1"), Some("https://example.com".into()));
        assert_eq!(origin("https://example.com:8443/login"), Some("https://example.com:8443".into()));
        assert_eq!(origin("http://127.0.0.1:3000/login"), Some("http://127.0.0.1:3000".into()));
        assert_eq!(origin("http://[::1]:3000/login"), Some("http://[::1]:3000".into()));
        let directory = tempfile::tempdir().unwrap();
        let vault = Vault { directory: directory.path().into(), secrets: MemorySecrets::default() };
        for url in ["http://example.com", "https://example.com/login", "https://example.com:443", "https://example.com.evil.test/path"] {
            assert!(vault.execute(Command::Save(login(url, "reader", "secret"))).is_err());
        }
        assert!(vault.secrets.0.lock().unwrap().is_empty());
    }
    #[test]
    fn failed_index_save_removes_new_system_credential() {
        struct BlockIndex {
            secrets: MemorySecrets,
            directory: PathBuf,
        }
        impl SecretStore for BlockIndex {
            fn read(&self, key: &str) -> Result<Zeroizing<String>, String> {
                self.secrets.read(key)
            }
            fn write(&self, key: &str, password: &str) -> Result<(), String> {
                self.secrets.write(key, password)?;
                fs::create_dir(self.directory.join("passwords.json")).unwrap();
                Ok(())
            }
            fn delete(&self, key: &str) -> Result<(), String> {
                self.secrets.delete(key)
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        let vault = Vault { directory: directory.path().into(), secrets: BlockIndex { secrets: secrets.clone(), directory: directory.path().into() } };
        assert!(vault.execute(Command::Save(login("https://example.com", "reader", "secret"))).is_err());
        assert!(secrets.0.lock().unwrap().is_empty());
    }
}
