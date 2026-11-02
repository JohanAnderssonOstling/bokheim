use super::AppClient;
use crate::runtime::AppCommand;

impl AppClient {
    pub async fn translate_selection(&self, text: String, source: String, target: String) -> Result<String, String> {
        // Reject oversized selections before transporting them to the worker.
        if text.len() > crate::translation::MAX_SELECTION_BYTES {
            return Err(crate::translation::SELECTION_TOO_LONG.into());
        }
        self.request::<Result<String, String>>(AppCommand::TranslateSelection { text, source, target }).await.map_err(|_| "Could not start translation. Try again.")?
    }
}
