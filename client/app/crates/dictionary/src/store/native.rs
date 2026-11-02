use std::fs::{self, File};
use std::path::{Path, PathBuf};

use client_platform_runtime::executor::run_blocking;

fn directory(root: &Path, id: &str) -> PathBuf {
    root.join("dictionaries-v1").join(id)
}

pub async fn write_all(root: &Path, id: &str, components: Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let path = directory(root, id);
    run_blocking(move || {
        fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        for (name, bytes) in components {
            fs::write(path.join(name), bytes).map_err(|error| error.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

pub async fn open(root: &Path, id: &str, name: &str) -> Result<File, String> {
    File::open(directory(root, id).join(name)).map_err(|error| error.to_string())
}

pub async fn remove(root: &Path, id: &str) -> Result<(), String> {
    let path = directory(root, id);
    run_blocking(move || match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    })
    .await
    .map_err(|error| error.to_string())?
}
