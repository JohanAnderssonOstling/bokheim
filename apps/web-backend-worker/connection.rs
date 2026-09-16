//! Shared application deployment configuration used by the web UI and worker probes.
pub fn configuration(broker_url: &str) -> bokheim_backend::host::web::BrowserConfiguration {
    // Coordinator identity survives deployment; dedicated workers use this tab's bundle.
    let stable = broker_url.rsplit_once("/pkg/").filter(|(_, suffix)| suffix.ends_with("/web_backend_worker_loader.js")).map(|(root, _)| format!("{root}/web_backend_worker_loader.js"));
    let base = broker_url.rsplit_once('/').map(|(base, _)| format!("{base}/")).unwrap_or_default();
    bokheim_backend::host::web::BrowserConfiguration {
        broker_url: stable.unwrap_or_else(|| broker_url.to_owned()),
        broker_name: "bokheim-backend-hosted-v3".into(),
        lifetime_lock: format!("bokheim-tab-{}", uuid::Uuid::new_v4()),
        worker_name_prefix: "bokheim-tab-worker".into(),
        workers: [("database", "web_backend_database_worker_loader.js?endpoint"), ("cpu", "cpu_worker_loader.js?endpoint"), ("import", "import_worker_loader.js?endpoint")]
            .into_iter()
            .map(|(role, script)| (role.into(), format!("{base}{script}")))
            .collect(),
    }
}
