use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

const SERVER_PACKAGES: [&str; 10] = ["server-account-core", "server-account-http", "server-account-mail", "server-account-postgres", "server-core", "server-database", "server-http", "server-postgres", "server-asset-store", "sync-server"];

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    dependencies: Vec<Dependency>,
    publish: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
}

fn metadata() -> Metadata {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let output = Command::new(env!("CARGO")).args(["metadata", "--format-version", "1", "--no-deps", "--manifest-path"]).arg(manifest).output().expect("cargo metadata must run");
    assert!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("cargo metadata must produce valid JSON")
}

#[test]
fn server_dependency_direction_is_enforced() {
    let packages: BTreeMap<_, _> = metadata().packages.into_iter().filter(|package| SERVER_PACKAGES.contains(&package.name.as_str())).map(|package| (package.name.clone(), package)).collect();
    assert_eq!(packages.len(), SERVER_PACKAGES.len(), "architecture policy must cover every server package");

    let allowed_internal: BTreeMap<&str, BTreeSet<&str>> = [
        ("server-account-core", BTreeSet::new()),
        ("server-account-http", BTreeSet::from(["server-account-core"])),
        ("server-account-mail", BTreeSet::from(["server-account-core"])),
        ("server-account-postgres", BTreeSet::from(["server-account-core", "server-database"])),
        ("server-core", BTreeSet::new()),
        ("server-database", BTreeSet::new()),
        ("server-http", BTreeSet::from(["server-account-core", "server-core"])),
        ("server-postgres", BTreeSet::from(["server-core", "server-database"])),
        ("server-asset-store", BTreeSet::from(["server-core"])),
        ("sync-server", BTreeSet::from(["server-account-core", "server-account-http", "server-account-mail", "server-account-postgres", "server-asset-store", "server-core", "server-database", "server-http", "server-postgres"])),
    ]
    .into_iter()
    .collect();

    for (name, package) in &packages {
        assert_eq!(package.publish.as_deref(), Some(&[][..]), "{name} must remain publish = false");
        let actual: BTreeSet<_> = package.dependencies.iter().map(|dependency| dependency.name.as_str()).filter(|dependency| SERVER_PACKAGES.contains(dependency)).collect();
        assert_eq!(actual, allowed_internal[name.as_str()], "forbidden internal dependency edge from {name}");
    }

    let forbidden_technology: BTreeMap<&str, BTreeSet<&str>> = [
        ("server-account-core", BTreeSet::from(["axum", "sqlx"])),
        ("server-account-http", BTreeSet::from(["sqlx"])),
        ("server-account-mail", BTreeSet::from(["axum", "sqlx"])),
        ("server-account-postgres", BTreeSet::from(["axum"])),
        ("server-core", BTreeSet::from(["axum", "sqlx"])),
        ("server-database", BTreeSet::from(["axum"])),
        ("server-http", BTreeSet::from(["sqlx"])),
        ("server-postgres", BTreeSet::from(["axum"])),
        ("server-asset-store", BTreeSet::from(["axum", "sqlx"])),
        ("sync-server", BTreeSet::from(["sqlx"])),
    ]
    .into_iter()
    .collect();

    for (name, forbidden) in forbidden_technology {
        let dependencies: BTreeSet<_> = packages[name].dependencies.iter().map(|dependency| dependency.name.as_str()).collect();
        let violations: Vec<_> = dependencies.intersection(&forbidden).copied().collect();
        assert!(violations.is_empty(), "{name} has forbidden technology dependencies: {violations:?}");
    }
}

#[test]
fn core_source_does_not_use_adapter_technologies() {
    fn rust_sources(directory: PathBuf, sources: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_sources(path, sources);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut sources = Vec::new();
    rust_sources(root.join("crates/server-core/src"), &mut sources);
    rust_sources(root.join("crates/server-account-core/src"), &mut sources);
    assert!(!sources.is_empty(), "server core crates must have Rust sources");
    for path in sources {
        let source = std::fs::read_to_string(&path).unwrap();
        for forbidden in ["axum::", "sqlx::", "std::fs", "tokio::fs", "server_postgres", "server_http", "server_asset_store"] {
            assert!(!source.contains(forbidden), "{} uses forbidden boundary token {forbidden:?}", path.display());
        }
    }
}

#[test]
fn account_and_sync_persistence_ownership_is_enforced() {
    fn combined_sources(path: PathBuf) -> String {
        std::fs::read_dir(path).unwrap().filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == "rs")).map(|path| std::fs::read_to_string(path).unwrap()).collect()
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sync_postgres = combined_sources(root.join("crates/server-postgres/src"));
    for forbidden in ["password_hash", "auth_token", "argon2", "verify_password", "mint_token"] {
        assert!(!sync_postgres.contains(forbidden), "synchronization persistence owns account token {forbidden:?}");
    }

    let account_postgres = combined_sources(root.join("crates/server-account-postgres/src"));
    for forbidden in ["library_events", "blob_object", "book_upload_reservation", "sync_common", "server_core"] {
        assert!(!account_postgres.contains(forbidden), "account persistence owns synchronization token {forbidden:?}");
    }
}

#[test]
fn synchronization_state_storage_remains_typed() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store = std::fs::read_to_string(root.join("crates/server-postgres/src/sync_store.rs")).unwrap();
    for forbidden in ["serde_json", "op.payload()", "Json(change", "library_events", "sync_state"] {
        assert!(!store.contains(forbidden), "typed synchronization storage regressed to generic event payload token {forbidden:?}");
    }
    let typed_migration = std::fs::read_to_string(root.join("crates/server-database/migrations/0023_typed_sync_state.sql")).unwrap();
    assert!(typed_migration.contains("ALTER TABLE sync_state DROP COLUMN data"), "typed-state migration must remove the JSONB payload column");
    let flattened_migration = std::fs::read_to_string(root.join("crates/server-database/migrations/0026_flatten_typed_sync_state.sql")).unwrap();
    let wall_clock_migration = std::fs::read_to_string(root.join("crates/server-database/migrations/0027_wall_clock_versions.sql")).unwrap();
    assert!(flattened_migration.contains("DROP TABLE sync_state CASCADE"), "flattened typed state must remove the shared envelope table");
    assert!(flattened_migration.contains("CREATE SEQUENCE sync_server_sequence"), "flattened typed state must preserve one global pull cursor");
    assert!(wall_clock_migration.contains("DROP COLUMN hlc_pt") && wall_clock_migration.contains("DROP COLUMN hlc_l"), "wall-clock migration must remove obsolete HLC storage");
    assert!(store.contains("include_str!(\"sql/sync/"), "synchronization SQL must be imported from dedicated SQL files");
    assert!(!store.contains("INSERT INTO sync_"), "synchronization writes must not be embedded in Rust strings");
    for table in [
        "sync_directory_state",
        "sync_book_lifecycle_state",
        "sync_book_placement_state",
        "sync_reading_position_state",
        "sync_annotation_state",
        "sync_bookmark_state",
        "sync_book_link_state",
        "sync_book_opened_state",
        "sync_book_metadata_state",
    ] {
        assert!(typed_migration.contains(&format!("CREATE TABLE {table}")), "typed-state migration is missing {table}");
        assert!(flattened_migration.contains(&format!("ALTER TABLE {table}")), "flattened typed state is missing {table}");
        assert!(flattened_migration.contains(&format!("ON {table}(library_id,server_seq)")), "flattened typed state is missing the pull index for {table}");
    }
}

#[test]
fn synchronization_server_does_not_own_opds_catalogs() {
    let metadata = metadata();
    for package in metadata.packages.into_iter().filter(|package| SERVER_PACKAGES.contains(&package.name.as_str())) {
        assert!(!package.dependencies.iter().any(|dependency| dependency.name == "bokheim-opds"), "{} must not depend on the OPDS crate", package.name);
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["src", "crates/server-core/src", "crates/server-http/src", "crates/server-postgres/src", "crates/server-asset-store/src"] {
        let mut sources = Vec::new();
        rust_sources_for_policy(root.join(relative), &mut sources);
        for path in sources {
            let source = std::fs::read_to_string(&path).unwrap().to_ascii_lowercase();
            assert!(!source.contains("opds"), "{} contains forbidden OPDS ownership", path.display());
        }
    }
}

fn rust_sources_for_policy(directory: PathBuf, sources: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources_for_policy(path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(path);
        }
    }
}

#[test]
fn synchronization_server_does_not_depend_on_or_serve_catalogs() {
    let metadata = metadata();
    for package in metadata.packages.into_iter().filter(|package| SERVER_PACKAGES.contains(&package.name.as_str())) {
        for dependency in package.dependencies {
            assert!(!matches!(dependency.name.as_str(), "bokheim-opds" | "bokheim-catalog-server"), "{} must not depend on the separate catalog service", package.name);
        }
    }
    let http_source = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/server-http/src/lib.rs")).unwrap();
    assert!(!http_source.contains("/api/opds/catalogs"), "the synchronization server must not expose the catalog endpoint");
}

#[test]
fn synchronization_compression_cannot_change_blob_bytes() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let router = std::fs::read_to_string(root.join("crates/server-http/src/lib.rs")).unwrap();
    let assets = std::fs::read_to_string(root.join("crates/server-http/src/assets.rs")).unwrap();
    let metadata_wire = std::fs::read_to_string(root.join("crates/server-wire-http/src/lib.rs")).unwrap();
    let wire_manifest = std::fs::read_to_string(root.join("../BokheimWire/Cargo.toml")).unwrap();
    assert!(!router.contains("CompressionLayer"), "global HTTP compression would also wrap immutable blob and range responses");
    assert!(!assets.contains("CONTENT_ENCODING"), "asset handlers must return the exact stored byte representation");
    assert!(metadata_wire.contains("CONTENT_ENCODING_ZSTD"), "compression must remain explicit at the versioned metadata boundary");
    assert!(wire_manifest.contains("bincode") && wire_manifest.contains("zstd"), "metadata wire must use the frozen binary and compression dependencies");
    assert!(!wire_manifest.contains("rmp-serde") && !wire_manifest.contains("flate2"), "the retired MessagePack and gzip protocol dependencies must not return");
}

#[test]
fn typed_http_protocol_cannot_reintroduce_json_transport() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for relative in ["crates/server-http/src", "crates/server-account-http/src"] {
        let mut sources = Vec::new();
        rust_sources_for_policy(root.join(relative), &mut sources);
        for source in sources {
            let contents = std::fs::read_to_string(&source).unwrap();
            assert!(!contents.contains("axum::Json") && !contents.contains("Json<") && !contents.contains("serde_json"), "{} must use the shared binary HTTP adapter", source.display());
        }
    }

    let client_source = std::fs::read_to_string(root.join("../BokheimSyncClient/src/auth.rs")).unwrap();
    let production_client_source = client_source.split("#[cfg(test)]").next().unwrap();
    assert!(!production_client_source.contains(".json(") && !production_client_source.contains("serde_json"), "the production synchronization client must not carry JSON over HTTP");

    let client_manifest = std::fs::read_to_string(root.join("../BokheimSyncClient/Cargo.toml")).unwrap();
    let server_http_manifest = std::fs::read_to_string(root.join("crates/server-http/Cargo.toml")).unwrap();
    assert!(!client_manifest.lines().any(|line| line.starts_with("reqwest") && line.contains("\"json\"")), "the client must not enable reqwest's JSON transport");
    assert!(!server_http_manifest.lines().any(|line| line.trim_start().starts_with("serde_json")), "the HTTP adapter must not depend on serde_json");
}
