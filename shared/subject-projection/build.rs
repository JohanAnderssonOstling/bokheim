use flate2::{Compression, write::GzEncoder};
use rusqlite::Connection;
use std::env;
use std::io::Write;
use std::path::PathBuf;
use subject_taxonomy::{runtime_index as runtime, taxonomy_reader};

#[path = "src/matcher_storage.rs"]
mod matcher_storage;
#[path = "src/runtime_format.rs"]
#[allow(dead_code)] // The shared contract also exposes constants used only by client loaders.
mod runtime_format;

fn main() {
    for path in ["build.rs", "src", "data"] {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=data/unified-taxonomy-v2.sqlite3");
    println!("cargo:rerun-if-env-changed=BOKHEIM_BUILD_TAXONOMY_SQLITE");
    println!("cargo:rerun-if-env-changed=BOKHEIM_BUILD_RUNTIME_TAXONOMY");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"));
    let runtime_input = env::var_os("BOKHEIM_BUILD_RUNTIME_TAXONOMY").map(PathBuf::from);
    let database_path = runtime_input.clone().or_else(|| env::var_os("BOKHEIM_BUILD_TAXONOMY_SQLITE").map(PathBuf::from)).unwrap_or_else(|| manifest_dir.join("data/unified-taxonomy-v2.sqlite3"));
    println!("cargo:rerun-if-changed={}", database_path.display());
    let connection = Connection::open_with_flags(&database_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap_or_else(|error| panic!("failed to open {}: {error}", database_path.display()));
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0)).expect("check taxonomy database integrity");
    assert_eq!(integrity, "ok", "taxonomy database failed its integrity check");
    let foreign_key_violations: i64 = connection.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |row| row.get(0)).expect("check taxonomy database foreign keys");
    assert_eq!(foreign_key_violations, 0, "taxonomy database contains invalid relationships");
    let format_version: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |row| row.get(0)).expect("taxonomy database has a format version");
    if runtime_input.is_some() {
        assert_eq!(format_version, runtime_format::FORMAT_VERSION.to_string(), "packaging requires the published runtime format");
        runtime_format::validate(&connection).expect("validate published runtime artifact");
    } else {
        assert_eq!(format_version, "1", "unsupported authoring taxonomy database format");
    }
    let published_release_id: u64 =
        connection.query_row("SELECT value FROM taxonomy_meta WHERE key='release_id'", [], |row| row.get::<_, String>(0)).expect("taxonomy database has a release identifier").parse().expect("taxonomy release identifier is an integer");
    assert!(published_release_id > 0, "taxonomy release identifier must be positive");

    let output_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let bisac_csv = std::fs::read_to_string(manifest_dir.join("data/bisac-2021.csv")).expect("read BISAC reference");
    let bisac_revision = blake3::hash(format!("bisac-reference-v1\n{bisac_csv}").as_bytes()).to_hex().to_string();
    let bisac_path = output_dir.join("bisac-reference.sqlite3");
    if bisac_path.exists() {
        std::fs::remove_file(&bisac_path).expect("replace generated BISAC reference");
    }
    subject_taxonomy::bisac_reference::compile(&bisac_path, &bisac_csv, &bisac_revision).expect("compile BISAC reference");
    let bisac_digest = blake3::hash(&std::fs::read(&bisac_path).expect("read compiled BISAC reference")).to_hex().to_string();
    // Finish the cache metadata before packaging. Native and web can install
    // this exact image without SQLite backups or a post-install write.
    let bisac = Connection::open(&bisac_path).expect("open BISAC reference metadata");
    bisac.execute("UPDATE reference_meta SET revision=?1", [&bisac_digest]).expect("record BISAC artifact identity");
    drop(bisac);
    let bisac_image = std::fs::read(&bisac_path).expect("read prepared BISAC reference");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
    encoder.write_all(&bisac_image).expect("compress BISAC reference");
    let compressed = encoder.finish().expect("finish BISAC reference compression");
    let compressed_digest = blake3::hash(&compressed).to_hex().to_string();
    std::fs::write(output_dir.join("bisac-reference.sqlite3.gz"), compressed).expect("write compressed BISAC reference");
    std::fs::write(output_dir.join("bisac-reference-gzip-digest.txt"), compressed_digest).expect("write compressed BISAC digest");
    std::fs::write(output_dir.join("bisac-reference-size.rs"), format!("{}usize", bisac_image.len())).expect("write BISAC reference size");
    std::fs::write(output_dir.join("bisac-reference-revision.txt"), bisac_revision).expect("write BISAC revision");
    std::fs::write(output_dir.join("bisac-reference-digest.txt"), bisac_digest).expect("write BISAC digest");
    let client_database_path = output_dir.join("unified-taxonomy-client-v2.sqlite3");
    if runtime_input.is_some() {
        std::fs::copy(&database_path, &client_database_path).unwrap_or_else(|error| panic!("failed to copy runtime to {}: {error}", client_database_path.display()));
    } else {
        connection.backup("main", &client_database_path, None).expect("copy a consistent authoring snapshot");
    }
    let flags = if runtime_input.is_some() { rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY } else { rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE };
    let client_connection = Connection::open_with_flags(&client_database_path, flags).unwrap_or_else(|error| panic!("failed to open {}: {error}", client_database_path.display()));
    if runtime_input.is_none() {
        // Route IDs identify occurrences, so a concept can retain several parents.
        client_connection.execute_batch(include_str!("src/route_storage.sql")).expect("compile curated navigation routes");
        let concepts = taxonomy_reader::read_unified_taxonomy_connection(&client_connection, "build-time taxonomy").expect("read bundled taxonomy");
        let subclasses = taxonomy_reader::read_lcc_subclasses(&client_connection).expect("read taxonomy subclass names");
        let matcher = runtime::build_runtime_with_subclasses(concepts, subclasses).expect("compile bundled taxonomy matcher");
        client_connection.execute_batch("BEGIN;").expect("begin matcher preparation");
        matcher_storage::prepare(&client_connection, &matcher).expect("prepare SQLite matching indexes");
        client_connection.execute_batch("COMMIT;").expect("commit bundled taxonomy indexes");
        runtime_format::finish(&client_connection).expect("finish bundled runtime artifact");
    }
    std::fs::write(output_dir.join("unified-taxonomy-revision-id.txt"), client_connection.query_row("SELECT value FROM taxonomy_meta WHERE key='client_revision'", [], |r| r.get::<_, String>(0)).expect("runtime content identity"))
        .expect("write taxonomy revision");
    std::fs::write(output_dir.join("taxonomy-release.rs"), format!("{published_release_id}u64")).expect("write taxonomy release");
    let image = std::fs::read(&client_database_path).expect("read prepared taxonomy");
    let digest = blake3::hash(&image).to_hex().to_string();
    std::fs::write(output_dir.join("taxonomy.digest"), &digest).expect("write taxonomy digest");
}
