//! Automatic sleep and power off are shared with Kobo's built-in reader.

use std::cell::Cell;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::time::Duration;

const NATIVE_CONFIG: &str = "/mnt/onboard/.kobo/Kobo/Kobo eReader.conf";
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const AUTO_SLEEP_MINUTES: [u32; 6] = [5, 10, 15, 30, 60, 0];
pub const AUTO_POWER_OFF_MINUTES: [u32; 6] = AUTO_SLEEP_MINUTES;

thread_local! {
    // Nickel is stopped while Bokheim runs, so read its preference once per launch.
    static IDLE_TIMEOUT: Cell<Option<Duration>> = Cell::new(load_idle_timeout(Path::new(NATIVE_CONFIG)));
    static POWER_OFF_TIMEOUT: Cell<Option<Duration>> = Cell::new(load_power_off_timeout(Path::new(NATIVE_CONFIG)));
}

pub fn idle_timeout() -> Option<Duration> {
    IDLE_TIMEOUT.with(Cell::get)
}

pub fn power_off_timeout() -> Option<Duration> {
    POWER_OFF_TIMEOUT.with(Cell::get)
}

/// Zero disables automatic sleep in both readers.
pub fn set_auto_sleep_minutes(minutes: u32) -> io::Result<()> {
    if !AUTO_SLEEP_MINUTES.contains(&minutes) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "Unsupported automatic sleep duration"));
    }
    save_power_timeout(Path::new(NATIVE_CONFIG), "AutoSleepMinutes", minutes)?;
    IDLE_TIMEOUT.with(|timeout| timeout.set((minutes != 0).then(|| Duration::from_secs(u64::from(minutes) * 60))));
    Ok(())
}

/// Zero disables automatic power off in both readers.
pub fn set_auto_power_off_minutes(minutes: u32) -> io::Result<()> {
    if !AUTO_POWER_OFF_MINUTES.contains(&minutes) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "Unsupported automatic power-off duration"));
    }
    save_power_timeout(Path::new(NATIVE_CONFIG), "AutoOffMinutes", minutes)?;
    POWER_OFF_TIMEOUT.with(|timeout| timeout.set((minutes != 0).then(|| Duration::from_secs(u64::from(minutes) * 60))));
    Ok(())
}

fn save_power_timeout(path: &Path, setting: &str, minutes: u32) -> io::Result<()> {
    let (config, permissions) = match fs::read_to_string(path) {
        Ok(config) => (config, Some(fs::metadata(path)?.permissions())),
        Err(error) if error.kind() == ErrorKind::NotFound => (String::new(), None),
        Err(error) => return Err(error),
    };
    let directory = path.parent().ok_or_else(|| io::Error::other("Kobo settings directory is unavailable"))?;
    let mut staged = tempfile::Builder::new().prefix(".bokheim-settings-").tempfile_in(directory)?;
    if let Some(permissions) = permissions {
        staged.as_file().set_permissions(permissions)?;
    }
    staged.write_all(update_power_timeout(&config, setting, minutes).as_bytes())?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

fn update_power_timeout(config: &str, setting: &str, minutes: u32) -> String {
    let newline = if config.contains("\r\n") { "\r\n" } else { "\n" };
    let mut output = String::with_capacity(config.len() + 64);
    let mut power_options = false;
    let mut insert_at = None;
    let mut found_key = false;
    for line in config.split_inclusive('\n') {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if let Some(section) = trimmed.strip_prefix('[').and_then(|section| section.strip_suffix(']')) {
            power_options = section.trim() == "PowerOptions";
            if power_options && insert_at.is_none() {
                insert_at = Some(output.len() + line.len());
            }
        }
        if power_options
            && let Some((key, _)) = line.split_once('=')
            && key.trim() == setting
        {
            let ending = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            output.push_str(&format!("{key}={minutes}{ending}"));
            found_key = true;
        } else {
            output.push_str(line);
        }
    }
    if !found_key {
        if let Some(position) = insert_at {
            let separator = if output[..position].ends_with('\n') { "" } else { newline };
            output.insert_str(position, &format!("{separator}{setting}={minutes}{newline}"));
        } else {
            if !output.is_empty() && !output.ends_with('\n') {
                output.push_str(newline);
            }
            output.push_str(&format!("[PowerOptions]{newline}{setting}={minutes}{newline}"));
        }
    }
    output
}

fn load_idle_timeout(path: &Path) -> Option<Duration> {
    match fs::read_to_string(path).and_then(|config| parse_idle_timeout(&config)) {
        Ok(timeout) => timeout,
        Err(error) => {
            if error.kind() != ErrorKind::NotFound {
                eprintln!("Could not read Kobo automatic sleep setting; using five minutes: {error}");
            }
            Some(DEFAULT_IDLE_TIMEOUT)
        }
    }
}

fn parse_idle_timeout(config: &str) -> io::Result<Option<Duration>> {
    parse_power_timeout(config, "AutoSleepMinutes", Some(DEFAULT_IDLE_TIMEOUT))
}

fn load_power_off_timeout(path: &Path) -> Option<Duration> {
    match fs::read_to_string(path).and_then(|config| parse_power_timeout(&config, "AutoOffMinutes", None)) {
        Ok(timeout) => timeout,
        Err(error) => {
            if error.kind() != ErrorKind::NotFound {
                eprintln!("Could not read Kobo automatic power-off setting; disabling automatic power off: {error}");
            }
            None
        }
    }
}

fn parse_power_timeout(config: &str, setting: &str, default: Option<Duration>) -> io::Result<Option<Duration>> {
    let mut power_options = false;
    let mut minutes = None;
    for line in config.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|line| line.strip_suffix(']')) {
            power_options = section.trim() == "PowerOptions";
            continue;
        }
        if power_options
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == setting
        {
            // Use the last occurrence of the native preference.
            minutes = Some(value.trim());
        }
    }
    let Some(minutes) = minutes else {
        return Ok(default);
    };
    let minutes = minutes.parse::<u32>().map_err(|_| io::Error::new(ErrorKind::InvalidData, format!("Kobo {setting} must be a non-negative integer")))?;
    Ok((minutes != 0).then(|| Duration::from_secs(u64::from(minutes) * 60)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_native_minutes_and_never() {
        for minutes in [0, 5, 10, 15, 30, 45, 60, 255] {
            let config = format!("[PowerOptions]\nAutoSleepMinutes={minutes}\n");
            let expected = (minutes != 0).then(|| Duration::from_secs(minutes * 60));
            assert_eq!(parse_idle_timeout(&config).unwrap(), expected);
        }
    }

    #[test]
    fn reads_only_the_native_sleep_key_in_power_options() {
        let config = "\u{feff}; Kobo preferences\r\n\
            AutoSleepMinutes=1\r\n\
            [Other]\r\nAutoSleepMinutes=2\r\n\
            [PowerOptions]\r\nAutoOffMinutes=60\r\n\
            # AutoSleepMinutes=3\r\n AutoSleepMinutes = 15 \r\n\
            [Reading]\r\nAutoSleepMinutes=4\r\n";
        assert_eq!(parse_idle_timeout(config).unwrap(), Some(Duration::from_secs(15 * 60)));
    }

    #[test]
    fn missing_sleep_preference_uses_the_fallback() {
        for config in ["", "[PowerOptions]\nAutoOffMinutes=60", "[Reading]\nAutoSleepMinutes=0"] {
            assert_eq!(parse_idle_timeout(config).unwrap(), Some(DEFAULT_IDLE_TIMEOUT));
        }
    }

    #[test]
    fn invalid_preferences_do_not_disable_automatic_sleep() {
        for value in ["", "-1", "never", "abc", "0.5", "4294967296"] {
            assert!(parse_idle_timeout(&format!("[PowerOptions]\nAutoSleepMinutes={value}")).is_err());
        }
    }

    #[test]
    fn native_configuration_is_read_again_at_launch_and_never_rewritten() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("Kobo eReader.conf");
        assert_eq!(load_idle_timeout(&path), Some(DEFAULT_IDLE_TIMEOUT));
        assert!(!path.exists());
        for (value, expected) in [("30", Some(Duration::from_secs(30 * 60))), ("0", None), ("invalid", Some(DEFAULT_IDLE_TIMEOUT))] {
            let config = format!("[PowerOptions]\nAutoSleepMinutes={value}\n[Other]\nKeep=unchanged\n");
            fs::write(&path, &config).unwrap();
            assert_eq!(load_idle_timeout(&path), expected);
            assert_eq!(fs::read_to_string(&path).unwrap(), config);
        }
    }
}
