use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

const SERVER_PACKAGES: [&str; 5] = ["server-core", "server-http", "server-postgres", "server-asset-store", "sync-server"];
const ADAPTERS: [&str; 3] = ["server-http", "server-postgres", "server-asset-store"];

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
        ("server-core", BTreeSet::new()),
        ("server-http", BTreeSet::from(["server-core"])),
        ("server-postgres", BTreeSet::from(["server-core"])),
        ("server-asset-store", BTreeSet::from(["server-core"])),
        ("sync-server", BTreeSet::from(ADAPTERS).into_iter().chain(["server-core"]).collect()),
    ]
    .into_iter()
    .collect();

    for (name, package) in &packages {
        assert_eq!(package.publish.as_deref(), Some(&[][..]), "{name} must remain publish = false");
        let actual: BTreeSet<_> = package.dependencies.iter().map(|dependency| dependency.name.as_str()).filter(|dependency| SERVER_PACKAGES.contains(dependency)).collect();
        assert_eq!(actual, allowed_internal[name.as_str()], "forbidden internal dependency edge from {name}");
    }

    let forbidden_technology: BTreeMap<&str, BTreeSet<&str>> = [
        ("server-core", BTreeSet::from(["axum", "sqlx"])),
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

    let mut sources = Vec::new();
    rust_sources(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/server-core/src"), &mut sources);
    assert!(!sources.is_empty(), "server-core must have Rust sources");
    for path in sources {
        let source = std::fs::read_to_string(&path).unwrap();
        for forbidden in ["axum::", "sqlx::", "std::fs", "tokio::fs", "server_postgres", "server_http", "server_asset_store"] {
            assert!(!source.contains(forbidden), "{} uses forbidden boundary token {forbidden:?}", path.display());
        }
    }
}
