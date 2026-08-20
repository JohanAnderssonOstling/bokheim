//! Device-local browser navigation state.

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::BrowserSession;

const BROWSER_SESSION_FILE: &str = "browser_session.json";

impl crate::app::BackendContext {
    fn browser_session_path(&self) -> PathBuf {
        self.app_data_dir().join(BROWSER_SESSION_FILE)
    }

    pub(crate) fn load_browser_session(&self) -> io::Result<BrowserSession> {
        match fs::read(self.browser_session_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BrowserSession::default()),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn save_browser_session(&self, session: &BrowserSession) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(session).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        super::persistence::write_json_atomic(&self.browser_session_path(), &bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppDataLocation;

    #[test]
    fn missing_session_uses_defaults_and_saved_session_round_trips() {
        let directory = tempfile::tempdir().unwrap();
        let context = crate::app::BackendContext::initialize(AppDataLocation::PlatformLocal(directory.path().to_path_buf())).unwrap();
        assert_eq!(context.load_browser_session().unwrap(), BrowserSession::default());

        let mut session = BrowserSession::default();
        session.active_library_id = Some(crate::LibraryId::from_u128(7));
        context.save_browser_session(&session).unwrap();

        assert_eq!(context.load_browser_session().unwrap(), session);
    }
}
