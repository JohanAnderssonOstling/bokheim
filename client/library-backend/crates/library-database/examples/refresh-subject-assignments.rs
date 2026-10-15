//! Rebuild stored subject assignments with the current matcher, without the UI.
use library_database::Database;
use std::{env, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let paths = env::args_os().skip(1).map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        eprintln!("Usage: refresh-subject-assignments LIBRARY_DATABASE...");
        return ExitCode::FAILURE;
    }
    for path in paths {
        if !path.is_file() {
            eprintln!("Library database does not exist: {}", path.display());
            return ExitCode::FAILURE;
        }
        match Database::open(&path).and_then(|db| {
            db.initialize_library()?;
            db.check_migrated()
        }) {
            Ok(_) => println!("Refreshed {}", path.display()),
            Err(error) => {
                eprintln!("{}: {error}", path.display());
                return ExitCode::FAILURE;
            }
        }
    }
    ExitCode::SUCCESS
}
