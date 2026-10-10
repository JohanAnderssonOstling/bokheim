//! Shared SQL primitives; callers retain transaction and policy ownership.
use content_address::ContentHash;
use include_sqlite_sql::include_sql;
include_sql!("src/sql/shared.sql");
