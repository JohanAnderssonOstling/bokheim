use crate::{error, MetadataError, MetadataService, Pool, QueryConnection, SqliteConnectionManager};
pub(crate) fn query_connection(pool: &Pool<SqliteConnectionManager>) -> Result<QueryConnection, MetadataError> {
    pool.get().map_err(error)
}
pub(crate) async fn run_database<T: Send + 'static>(service: MetadataService, operation: impl FnOnce(&MetadataService) -> Result<T, MetadataError> + Send + 'static) -> Result<Result<T, MetadataError>, tokio::task::JoinError> {
    tokio::task::spawn_blocking(move || operation(&service)).await
}
