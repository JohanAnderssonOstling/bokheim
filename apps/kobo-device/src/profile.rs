//! Runtime identity. USB installation uses the same profile parser.
pub use kobo_profile::{Frontlight, Model, PROFILES, Profile, TouchProtocol};
use std::{env, fs, io};

pub fn current() -> io::Result<&'static Profile> {
    let product = env::var("PRODUCT").unwrap_or_default();
    let version = match fs::read_to_string("/mnt/onboard/.kobo/version") {
        Ok(version) => version,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    Profile::detect(&product, &version).map_err(|error| io::Error::new(io::ErrorKind::Unsupported, error))
}
