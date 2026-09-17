use crate::storage_values::bounded_u8;
use crate::storage_values::invalid_text_column;
use crate::storage_values::nonnegative_u64;
use crate::storage_values::parse_uuid_column;
use library_replica::VersionKey;
use rusqlite::Connection;
use sync_common::MutationId;
use sync_common::ReplicaId;

pub fn get_or_create_replica_id(db: &Connection) -> Result<ReplicaId, rusqlite::Error> {
    match super::get_sync_metadata(db, "replica_id")? {
        Some(id) => parse_uuid_column(id, 0),
        None => {
            let new_id = ReplicaId::new_v4();
            super::set_sync_metadata(db, "replica_id", &new_id.to_string())?;
            Ok(new_id)
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StoredVersion(VersionKey);

impl StoredVersion {
    fn decode(changed_at: i64, conflict_rank: i64, actor: String, actor_seq: i64, mutation_id: String, columns: [usize; 5]) -> rusqlite::Result<Option<Self>> {
        if changed_at == 0 && actor.is_empty() && actor_seq == 0 && mutation_id.is_empty() {
            return Ok(None);
        }
        let mutation_id = MutationId::parse(&mutation_id).map_err(|error| invalid_text_column(columns[4], error.to_string()))?;
        let changed_at = nonnegative_u64(changed_at, columns[0], "change timestamp")?;
        let conflict_rank = bounded_u8(conflict_rank, columns[1], "conflict rank")?;
        let actor_seq = nonnegative_u64(actor_seq, columns[3], "actor sequence")?;
        let actor = parse_uuid_column(actor, columns[2])?;
        Ok(Some(Self(VersionKey::new(changed_at, conflict_rank, actor, actor_seq, mutation_id))))
    }

    fn into_key(self) -> VersionKey {
        self.0
    }

    pub(crate) fn decode_row(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<Option<VersionKey>> {
        Self::decode(row.get(offset)?, row.get(offset + 1)?, row.get(offset + 2)?, row.get(offset + 3)?, row.get(offset + 4)?, [offset, offset + 1, offset + 2, offset + 3, offset + 4]).map(|version| version.map(Self::into_key))
    }
}
