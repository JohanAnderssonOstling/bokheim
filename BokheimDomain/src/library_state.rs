use bokheim_algorithms::{Contributor, MarcRelatorCode, PublicationMetadata, ReadingPosition};
use bokheim_identifiers::UnixMillis;
use serde::{Deserialize, Serialize};

use crate::BookFormat;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BookLifecycleState { Present { metadata: BookMetadataState, added_at: UnixMillis, format: BookFormat }, Deleted }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadingPositionState { pub location: ReadingPosition, pub progress: f32 }
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BookMetadataState { pub title: String, pub contributors: Vec<Contributor>, #[serde(default)] pub description: String, #[serde(default)] pub publication: PublicationMetadata }
impl BookMetadataState { pub fn authors(&self) -> impl Iterator<Item = &Contributor> { self.contributors.iter().filter(|contributor| contributor.role_code() == MarcRelatorCode(*b"aut")) } }
