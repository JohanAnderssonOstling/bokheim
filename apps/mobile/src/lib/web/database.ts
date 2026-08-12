import sqlite3InitModule, { type Database, type Sqlite3Static } from "@sqlite.org/sqlite-wasm";

const WEB_SCHEMA_VERSION = 6;
const WEB_SCHEMA = `
  CREATE TABLE IF NOT EXISTS web_backend_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
  ) STRICT;
  INSERT INTO web_backend_metadata (key, value)
  VALUES ('schema_version', '${WEB_SCHEMA_VERSION}')
  ON CONFLICT (key) DO UPDATE SET value = excluded.value;

  CREATE TABLE IF NOT EXISTS libraries (
    library_id TEXT PRIMARY KEY NOT NULL,
    library_name TEXT NOT NULL CHECK (length(trim(library_name)) BETWEEN 1 AND 255),
    created_at INTEGER NOT NULL
  ) STRICT;

  CREATE TABLE IF NOT EXISTS books (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    file_name TEXT NOT NULL,
    title TEXT NOT NULL,
    author TEXT NOT NULL DEFAULT '',
    author_id TEXT,
    author_key TEXT NOT NULL DEFAULT '',
    cover_bytes BLOB,
    storage_key TEXT,
    bytes BLOB,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, content_hash)
  ) STRICT;

  CREATE TABLE IF NOT EXISTS directories (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    directory_id TEXT NOT NULL,
    parent_id TEXT NOT NULL,
    name TEXT NOT NULL COLLATE NOCASE CHECK (length(trim(name)) BETWEEN 1 AND 255),
    created_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, directory_id),
    UNIQUE (library_id, parent_id, name)
  ) STRICT;

  CREATE TABLE IF NOT EXISTS book_placements (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    directory_id TEXT NOT NULL,
    file_name TEXT NOT NULL,
    added_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, content_hash, directory_id),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS reading_positions (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    cfi TEXT NOT NULL,
    progress REAL,
    modified_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, content_hash),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_preferences (
    key TEXT PRIMARY KEY NOT NULL,
    value_json TEXT NOT NULL CHECK (json_valid(value_json))
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_sync_state (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    replica_id TEXT NOT NULL,
    last_replica_seq INTEGER NOT NULL DEFAULT 0 CHECK (last_replica_seq >= 0),
    last_server_revision INTEGER CHECK (last_server_revision IS NULL OR last_server_revision > 0),
    last_synced_at INTEGER
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_sync_outbox (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    mutation_id TEXT PRIMARY KEY NOT NULL,
    replica_seq INTEGER NOT NULL CHECK (replica_seq > 0),
    kind TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    entity_subkey TEXT NOT NULL DEFAULT '',
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    changed_at INTEGER NOT NULL,
    UNIQUE (library_id, replica_seq)
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_sync_cells (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    entity_key TEXT NOT NULL,
    entity_subkey TEXT NOT NULL DEFAULT '',
    value_json TEXT NOT NULL CHECK (json_valid(value_json)),
    changed_at INTEGER NOT NULL,
    conflict_rank INTEGER NOT NULL,
    replica_id TEXT NOT NULL,
    replica_seq INTEGER NOT NULL,
    mutation_id TEXT NOT NULL,
    PRIMARY KEY (library_id, kind, entity_key, entity_subkey)
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_account_profile (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    server_url TEXT NOT NULL,
    user_id TEXT NOT NULL,
    email TEXT NOT NULL
  ) STRICT;

  -- Browser bearer tokens were used during the prototype. Cookies are now
  -- authoritative, so remove any legacy JavaScript-readable credentials.
  DROP TABLE IF EXISTS web_account_session;

  CREATE TABLE IF NOT EXISTS web_library_tombstones (
    library_id TEXT PRIMARY KEY NOT NULL,
    deleted_at INTEGER NOT NULL
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_remote_libraries (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_transfers (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('book', 'thumbnail')),
    direction TEXT NOT NULL CHECK (direction IN ('upload', 'download')),
    state TEXT NOT NULL CHECK (state IN ('queued', 'running', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, content_hash, kind, direction)
  ) STRICT;

`;

export interface WebDatabase {
  sqlite3: Sqlite3Static;
  db: Database;
}

export interface WebDatabaseHealth {
  sqlite_version: string;
  schema_version: number;
  persistence: "memory" | "opfs";
}

let databasePromise: Promise<WebDatabase> | undefined;

export function initializeWebDatabase(db: Database): void {
  db.exec("PRAGMA foreign_keys = ON");
  db.exec(WEB_SCHEMA);
}

async function openDatabase(): Promise<WebDatabase> {
  const sqlite3 = await sqlite3InitModule();
  const db = "opfs" in sqlite3
    ? new sqlite3.oo1.OpfsDb("/bokheim-web-v4.sqlite3", "c")
    : new sqlite3.oo1.DB(":memory:", "c");
  initializeWebDatabase(db);
  return { sqlite3, db };
}

export function getWebDatabase(): Promise<WebDatabase> {
  return databasePromise ??= openDatabase();
}

export async function getWebDatabaseHealth(): Promise<WebDatabaseHealth> {
  const { db, sqlite3 } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT value FROM web_backend_metadata WHERE key = 'schema_version'",
    rowMode: 0,
    returnValue: "resultRows",
  });
  const schemaVersion = Number(rows[0]);
  if (!Number.isSafeInteger(schemaVersion)) throw new Error("The web database schema version is invalid");
  return {
    sqlite_version: sqlite3.version.libVersion,
    schema_version: schemaVersion,
    persistence: "opfs" in sqlite3 ? "opfs" : "memory",
  };
}
