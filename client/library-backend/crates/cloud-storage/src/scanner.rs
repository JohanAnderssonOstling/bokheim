//! One-page recursive scans. The caller persists the returned entries and
//! continuation together; a failure leaves the previous committed cursor valid.

use crate::{CloudEntry, CloudError, CloudStorage};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use sync_common::storage::ProviderId;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ScanCursor {
    root: ProviderId,
    folders: VecDeque<ProviderId>,
    discovered: HashSet<ProviderId>,
    page: Option<String>,
}

pub struct ScanPage {
    pub folder: ProviderId,
    pub entries: Vec<CloudEntry>,
    /// `None` is the only successful end-of-inventory marker. The caller must
    /// still reconcile provider changes captured during the full listing.
    pub next: Option<ScanCursor>,
}

impl ScanCursor {
    pub fn new(root: ProviderId) -> Self {
        Self { folders: VecDeque::from([root.clone()]), discovered: HashSet::from([root.clone()]), root, page: None }
    }

    pub fn root(&self) -> &ProviderId { &self.root }

    pub async fn next_page(&self, provider: &dyn CloudStorage, token: &str) -> Result<ScanPage, CloudError> {
        let folder = self.folders.front().ok_or_else(|| CloudError::InvalidRequest("Cloud scan has no pending folder".into()))?;
        let result = provider.list_folder(token, folder, self.page.as_deref(), false).await?;
        let mut next = self.clone();
        let mut entries = Vec::new();
        for entry in result.entries {
            let parents = match &entry { CloudEntry::Folder(folder) => &folder.parent_ids, CloudEntry::File(file) => &file.parent_ids };
            if !parents.contains(folder) { return Err(CloudError::InvalidResponse("Cloud listing contains an entry from another folder".into())); }
            if let CloudEntry::Folder(child) = &entry {
                // Internal thumbnails and transfer bookkeeping are not books.
                if child.name == ".bokheim" { continue; }
                if next.discovered.insert(child.id.clone()) { next.folders.push_back(child.id.clone()); }
            }
            entries.push(entry);
        }
        next.page = result.next_page;
        if next.page.is_none() { next.folders.pop_front(); }
        Ok(ScanPage { folder: folder.clone(), entries, next: (!next.folders.is_empty()).then_some(next) })
    }
}
