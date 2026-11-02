//! Register sideloaded faces before resolving saved interface or book fonts.

use std::{borrow::Cow, fs, io::ErrorKind, path::PathBuf};

use gpui::App;

pub(super) fn register(cx: &App) {
    let mut directories = vec![PathBuf::from("/mnt/onboard/fonts")];
    while let Some(directory) = directories.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                if error.kind() != ErrorKind::NotFound {
                    log::warn!("Could not read Kobo font directory {}: {error}", directory.display());
                }
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    log::warn!("Could not read Kobo font entry: {error}");
                    continue;
                }
            };
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else { continue };
            if file_type.is_dir() {
                directories.push(path);
                continue;
            }
            if !file_type.is_file() || !path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| ["ttf", "otf", "ttc", "otc"].iter().any(|supported| extension.eq_ignore_ascii_case(supported))) {
                continue;
            }
            match fs::read(&path) {
                Ok(bytes) => {
                    if let Err(error) = cx.text_system().add_fonts(vec![Cow::Owned(bytes)]) {
                        log::warn!("Could not register Kobo font {}: {error}", path.display());
                    }
                }
                Err(error) => log::warn!("Could not read Kobo font {}: {error}", path.display()),
            }
        }
    }
}
