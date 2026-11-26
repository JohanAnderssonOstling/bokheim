//! Permanent successful provider responses and completed recording decisions.
use metadata_contract::audible::{LookupResponse, LookupStatus};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(super) struct ResponseCache {
    database: Arc<Mutex<Connection>>,
}

impl ResponseCache {
    pub(super) fn new(database: Arc<Mutex<Connection>>) -> Self {
        Self { database }
    }

    pub(super) async fn get(&self, url: &str) -> Result<Option<Arc<Vec<u8>>>, String> {
        let database = self.database.clone();
        let url = url.to_owned();
        tokio::task::spawn_blocking(move || {
            let connection = database.lock().map_err(|error| error.to_string())?;
            connection.query_row("SELECT bytes FROM audnexus_response WHERE url=?1", [url], |row| row.get::<_, Vec<u8>>(0)).optional().map(|bytes| bytes.map(Arc::new)).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub(super) async fn put(&self, url: String, bytes: Arc<Vec<u8>>) -> Result<(), String> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || {
            let connection = database.lock().map_err(|error| error.to_string())?;
            connection.execute("INSERT INTO audnexus_response(url,bytes) VALUES(?1,?2) ON CONFLICT(url) DO NOTHING", params![url, bytes.as_slice()]).map(|_| ()).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub(super) async fn get_match(&self, key: [u8; 32], request: Arc<Vec<u8>>) -> Result<Option<LookupResponse>, String> {
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || {
            let connection = database.lock().map_err(|error| error.to_string())?;
            // Compare the full input too: hashes only retrieve records.
            let bytes = connection.query_row("SELECT response FROM recording_match WHERE key=?1 AND request=?2", params![key.as_slice(), request.as_slice()], |row| row.get::<_, Vec<u8>>(0)).optional().map_err(|error| error.to_string())?;
            bytes.map(|bytes| serde_json::from_slice::<LookupResponse>(&bytes).map_err(|error| error.to_string())).transpose().map(|response| response.filter(|response| response.status == LookupStatus::Supported))
        })
        .await
        .map_err(|error| error.to_string())?
    }

    pub(super) async fn put_match(&self, key: [u8; 32], request: Arc<Vec<u8>>, response: LookupResponse) -> Result<(), String> {
        if response.status != LookupStatus::Supported {
            return Ok(());
        }
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || {
            let bytes = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
            let connection = database.lock().map_err(|error| error.to_string())?;
            connection.execute("INSERT INTO recording_match(key,request,response) VALUES(?1,?2,?3) ON CONFLICT(key) DO NOTHING", params![key.as_slice(), request.as_slice(), bytes]).map(|_| ()).map_err(|error| error.to_string())
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
