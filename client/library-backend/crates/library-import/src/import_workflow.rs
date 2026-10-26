//! Sequential import orchestration shared by direct readers and durable staging.
//! Adapters own source access and checkpoints.

use crate::{ImportFailure, ImportFailureKind};
use std::collections::BTreeMap;
use sync_common::DirId;

pub trait Batch {
    type Error;
    fn len(&self) -> usize;
    async fn process(&mut self, index: usize) -> Result<(), Self::Error>;
}

pub async fn run<B: Batch>(batch: &mut B) -> Result<(), B::Error> {
    for index in 0..batch.len() {
        batch.process(index).await?;
    }
    Ok(())
}

pub fn file_failure(path: &[String], error: &str) -> ImportFailure {
    log::warn!("Could not import {}: {error}", path.join("/"));
    ImportFailure::operation(ImportFailureKind::File, path, error)
}

pub fn folder_failure(path: &[String], error: &str) -> ImportFailure {
    log::warn!("Could not import folder {}: {error}", path.join("/"));
    ImportFailure::operation(ImportFailureKind::Folder, path, error)
}

#[derive(Default)]
pub struct Destinations(BTreeMap<Vec<String>, DirId>);

impl Destinations {
    pub fn new(directories: Vec<(Vec<String>, DirId)>) -> Self {
        Self(directories.into_iter().collect())
    }

    pub fn resolve<'a>(&self, path: &'a [String]) -> Result<(DirId, &'a str), String> {
        let (name, parent) = path.split_last().ok_or("import file path is empty")?;
        let parent = *self.0.get(parent).ok_or("Parent folder could not be imported")?;
        Ok((parent, name))
    }
}

pub fn supported(path: &[String]) -> bool {
    path.last().is_some_and(|name| book_model::BookFormat::from_file_name(name).is_some())
}
