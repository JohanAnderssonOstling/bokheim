//! Cache the bundled BISAC reference in this worker's OPFS-backed SQLite VFS.
use client_platform_web::sqlite;
use subject_projection::{bisac_reference_filename, install_bisac_reference, open_bisac_reference};

pub async fn install() -> Result<(), String> {
    let filename = bisac_reference_filename();
    sqlite::prune_reference_databases("bisac-reference", &filename)?;
    if open_bisac_reference()? {
        return Ok(());
    }
    let image = super::download_bundle(&filename).await?;
    install_bisac_reference(&image, |path, image| {
        let name = path.to_str().ok_or("BISAC reference path is not UTF-8")?;
        sqlite::import_reference_database(name, image)
    })
}
