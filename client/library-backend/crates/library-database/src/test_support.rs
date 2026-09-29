use crate::Database;

pub(crate) struct TestDatabase {
    pub(crate) database: Database,
    pub(crate) path: std::path::PathBuf,
}

impl TestDatabase {
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("library-database-{name}-{}", uuid::Uuid::new_v4()));
        let database = Database::open(&path).expect("open test database");
        database.initialize_library().expect("initialize test library");
        Self { database, path }
    }
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
