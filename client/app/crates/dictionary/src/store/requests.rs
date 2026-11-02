//! Execute dictionary requests without involving application command routing.
use super::DictionaryStore;
use crate::{Request, Response, Result};
use std::sync::Arc;

impl DictionaryStore {
    pub async fn execute(self: &Arc<Self>, request: Request) -> Result<Response> {
        Ok(match request {
            Request::List => Response::Dictionaries(self.list().await?),
            Request::Snapshot { languages } => Response::Snapshot(self.snapshot(&languages).await?),
            Request::Downloads { languages } => Response::Downloads(self.downloads(&languages).await?),
            Request::Download { id } => Response::Dictionaries(self.download(id).await?),
            Request::Configure { id, enabled, direction } => Response::Dictionaries(self.configure(id, enabled, direction).await?),
            Request::Remove { id } => Response::Dictionaries(self.remove(id).await?),
        })
    }
}
