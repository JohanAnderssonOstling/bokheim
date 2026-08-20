import sqlite3InitModule, {
  type Database,
  type Sqlite3Static,
} from "@sqlite.org/sqlite-wasm";

const WEB_SCHEMA_VERSION = 15;
const WEB_SCHEMA = `
  CREATE TABLE IF NOT EXISTS web_backend_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
  ) STRICT;
  INSERT INTO web_backend_metadata (key, value)
  VALUES ('schema_version', '${WEB_SCHEMA_VERSION}')
  ON CONFLICT (key) DO UPDATE SET value = excluded.value;

  CREATE TABLE IF NOT EXISTS web_taxonomy_concepts (
    concept_id TEXT PRIMARY KEY NOT NULL,
    concept_json TEXT NOT NULL CHECK (json_valid(concept_json))
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_taxonomy_resolutions (
    system_id TEXT NOT NULL CHECK (system_id IN ('bisac', 'ddc', 'lcc')),
    code TEXT NOT NULL,
    server_release_id TEXT NOT NULL,
    matched_concept_ids_json TEXT NOT NULL CHECK (json_valid(matched_concept_ids_json)),
    PRIMARY KEY (system_id, code)
  ) STRICT;

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
    description TEXT NOT NULL DEFAULT '',
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

  CREATE TABLE IF NOT EXISTS book_subject_paths (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    subject_path TEXT NOT NULL,
    PRIMARY KEY (library_id, content_hash, subject_path),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS book_toc_cache (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    toc_json TEXT NOT NULL CHECK (json_valid(toc_json)),
    PRIMARY KEY (library_id, content_hash),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_book_identifiers (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    scheme TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (library_id, content_hash, scheme, value),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_author_external_identifiers (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    authority TEXT NOT NULL,
    external_id TEXT NOT NULL,
    author_id TEXT NOT NULL,
    preferred_name TEXT NOT NULL,
    PRIMARY KEY (library_id, authority, external_id)
  ) STRICT;
  CREATE INDEX IF NOT EXISTS idx_web_author_external_identity
    ON web_author_external_identifiers(library_id, author_id);

  CREATE TABLE IF NOT EXISTS web_external_metadata_attempts (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    identifier TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('updated', 'no_match', 'ambiguous', 'failed')),
    detail TEXT,
    attempted_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, content_hash, provider_id),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_external_entities (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    wikidata_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('place', 'person')),
    label TEXT NOT NULL,
    aliases_json TEXT NOT NULL CHECK (json_valid(aliases_json)),
    birth_year INTEGER,
    death_year INTEGER,
    fictional INTEGER NOT NULL DEFAULT 0 CHECK (fictional IN (0, 1)),
    latitude_e7 INTEGER,
    longitude_e7 INTEGER,
    PRIMARY KEY (library_id, wikidata_id)
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_external_place_edges (
    library_id TEXT NOT NULL,
    child_id TEXT NOT NULL,
    parent_id TEXT NOT NULL,
    relation TEXT NOT NULL CHECK (relation IN ('parent', 'country')),
    PRIMARY KEY (library_id, child_id, parent_id, relation),
    FOREIGN KEY (library_id, child_id) REFERENCES web_external_entities(library_id, wikidata_id) ON DELETE CASCADE,
    FOREIGN KEY (library_id, parent_id) REFERENCES web_external_entities(library_id, wikidata_id) ON DELETE CASCADE
  ) STRICT;
  CREATE INDEX IF NOT EXISTS idx_web_external_place_parent ON web_external_place_edges(library_id, parent_id, child_id);

  CREATE TABLE IF NOT EXISTS web_book_external_entities (
    library_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    wikidata_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('place', 'person')),
    evidence_name TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('exact_edition', 'work')),
    PRIMARY KEY (library_id, content_hash, wikidata_id, kind, evidence_name),
    FOREIGN KEY (library_id, content_hash) REFERENCES books(library_id, content_hash) ON DELETE CASCADE,
    FOREIGN KEY (library_id, wikidata_id) REFERENCES web_external_entities(library_id, wikidata_id) ON DELETE CASCADE
  ) STRICT;
  CREATE INDEX IF NOT EXISTS idx_web_book_external_entity ON web_book_external_entities(library_id, kind, wikidata_id, content_hash);

  CREATE TABLE IF NOT EXISTS web_catalog_publications (
    library_id TEXT NOT NULL,
    source_id TEXT NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL DEFAULT '',
    content_hash TEXT NOT NULL,
    PRIMARY KEY (library_id, source_id, edition_id, work_id),
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
    last_state_revision INTEGER CHECK (last_state_revision IS NULL OR last_state_revision > 0),
    last_reading_revision INTEGER CHECK (last_reading_revision IS NULL OR last_reading_revision > 0),
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
    deleted_at INTEGER NOT NULL,
    account_id TEXT
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_remote_libraries (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_library_owners (
    library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    user_id TEXT NOT NULL
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

  CREATE TABLE IF NOT EXISTS web_trash_books (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    title TEXT NOT NULL,
    format TEXT NOT NULL,
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, content_hash)
  ) STRICT;

  CREATE TABLE IF NOT EXISTS web_trash_folders (
    library_id TEXT NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
    directory_id TEXT NOT NULL,
    name TEXT NOT NULL,
    deleted_at INTEGER NOT NULL,
    PRIMARY KEY (library_id, directory_id)
  ) STRICT;

`;

export interface WebDatabase {
  sqlite3: Sqlite3Static;
  db: Database;
  persistence: "memory" | "opfs" | "indexeddb";
}

export interface WebDatabaseHealth {
  sqlite_version: string;
  schema_version: number;
  persistence: "memory" | "opfs" | "indexeddb";
}

let databasePromise: Promise<WebDatabase> | undefined;
const BACKUP_DATABASE_NAME = "bokheim-web-database";
const BACKUP_STORE_NAME = "sqlite";
const BACKUP_KEY = "bokheim-web-v4.sqlite3";
const OPFS_BACKUP_KEY = "bokheim-web-v4.backup.sqlite3";

function unavailableFileStorage(reason: unknown): boolean {
  return (
    reason instanceof DOMException &&
    [
      "SecurityError",
      "NotAllowedError",
      "NotSupportedError",
      "InvalidStateError",
    ].includes(reason.name)
  );
}

async function backupFile(create: boolean): Promise<FileSystemFileHandle | null> {
  const storage = (globalThis.navigator as Navigator | undefined)?.storage;
  if (!storage?.getDirectory) return null;
  try {
    const root = await storage.getDirectory();
    return await root.getFileHandle(OPFS_BACKUP_KEY, { create });
  } catch (reason) {
    if (!create && reason instanceof DOMException && reason.name === "NotFoundError")
      return null;
    if (unavailableFileStorage(reason)) return null;
    throw reason;
  }
}

function indexedDbBackupDatabase(): Promise<IDBDatabase | null> {
  if (!("indexedDB" in globalThis)) return Promise.resolve(null);
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(BACKUP_DATABASE_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(BACKUP_STORE_NAME);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error("Could not open the browser database backup"));
  });
}

async function loadDatabaseBackup(): Promise<Uint8Array | null> {
  const file = await backupFile(false);
  if (file) {
    const stored = await file.getFile();
    // Creating a file handle precedes its first writable stream. If that
    // initial write was aborted (quota exhaustion, process termination), an
    // empty placeholder may remain and must not shadow the older IndexedDB
    // backup.
    if (stored.size > 0) return new Uint8Array(await stored.arrayBuffer());
  }

  const backup = await indexedDbBackupDatabase();
  if (!backup) return null;
  try {
    return await new Promise((resolve, reject) => {
      const request = backup
        .transaction(BACKUP_STORE_NAME)
        .objectStore(BACKUP_STORE_NAME)
        .get(BACKUP_KEY);
      request.onsuccess = () => {
        const value = request.result;
        resolve(
          value instanceof Uint8Array
            ? value
            : value instanceof ArrayBuffer
              ? new Uint8Array(value)
              : null,
        );
      };
      request.onerror = () =>
        reject(request.error ?? new Error("Could not read the browser database backup"));
    });
  } finally {
    backup.close();
  }
}

async function saveDatabaseBackup(bytes: Uint8Array): Promise<boolean> {
  const file = await backupFile(true);
  if (file) {
    const writable = await file.createWritable();
    try {
      await writable.write(bytes.slice().buffer);
      await writable.close();
      return true;
    } catch (reason) {
      await writable.abort().catch(() => undefined);
      throw reason;
    }
  }

  const backup = await indexedDbBackupDatabase();
  if (!backup) return false;
  try {
    await new Promise<void>((resolve, reject) => {
      const transaction = backup.transaction(BACKUP_STORE_NAME, "readwrite");
      const request = transaction.objectStore(BACKUP_STORE_NAME).put(bytes, BACKUP_KEY);
      request.onerror = () =>
        reject(request.error ?? new Error("Could not save the browser database backup"));
      transaction.oncomplete = () => resolve();
      transaction.onerror = () =>
        reject(
          transaction.error ?? new Error("Could not save the browser database backup"),
        );
      transaction.onabort = () =>
        reject(
          transaction.error ??
            new Error("Saving the browser database backup was aborted"),
        );
    });
    return true;
  } finally {
    backup.close();
  }
}

export function initializeWebDatabase(db: Database): void {
  db.exec("PRAGMA foreign_keys = ON");
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec(WEB_SCHEMA);
    const bookColumns = new Set(
      db
        .exec({
          sql: "SELECT name FROM pragma_table_info('books')",
          rowMode: 0,
          returnValue: "resultRows",
        })
        .map(String),
    );
    if (!bookColumns.has("description")) {
      db.exec("ALTER TABLE books ADD COLUMN description TEXT NOT NULL DEFAULT ''");
    }
    const tombstoneColumns = new Set(
      db
        .exec({
          sql: "SELECT name FROM pragma_table_info('web_library_tombstones')",
          rowMode: 0,
          returnValue: "resultRows",
        })
        .map(String),
    );
    if (!tombstoneColumns.has("account_id")) {
      db.exec("ALTER TABLE web_library_tombstones ADD COLUMN account_id TEXT");
    }
    const syncColumns = new Set(
      db
        .exec({
          sql: "SELECT name FROM pragma_table_info('web_sync_state')",
          rowMode: 0,
          returnValue: "resultRows",
        })
        .map(String),
    );
    if (!syncColumns.has("last_state_revision")) {
      db.exec(
        "ALTER TABLE web_sync_state ADD COLUMN last_state_revision INTEGER CHECK (last_state_revision IS NULL OR last_state_revision > 0)",
      );
      db.exec("UPDATE web_sync_state SET last_state_revision=last_server_revision");
    }
    if (!syncColumns.has("last_reading_revision")) {
      db.exec(
        "ALTER TABLE web_sync_state ADD COLUMN last_reading_revision INTEGER CHECK (last_reading_revision IS NULL OR last_reading_revision > 0)",
      );
      db.exec("UPDATE web_sync_state SET last_reading_revision=last_server_revision");
    }
    // Legacy tombstones have no trustworthy account identity. Retaining them
    // could apply one user's deletion after a different user signs in.
    db.exec("DELETE FROM web_library_tombstones WHERE account_id IS NULL");
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

async function openDatabase(): Promise<WebDatabase> {
  const sqlite3 = await sqlite3InitModule();
  let db: Database;
  let persistence: WebDatabase["persistence"];
  try {
    if ("opfs" in sqlite3) {
      db = new sqlite3.oo1.OpfsDb("/bokheim-web-v4.sqlite3", "c");
    } else {
      // The access-handle-pool VFS does not require the SharedArrayBuffer
      // proxy used by OpfsDb. This keeps books persistent in browsers which
      // expose OPFS but cannot initialize that older backend.
      const pool = await sqlite3.installOpfsSAHPoolVfs({
        name: "bokheim-opfs-sahpool-v1",
        directory: ".bokheim-opfs-sahpool-v1",
        initialCapacity: 6,
      });
      db = new pool.OpfsSAHPoolDb("/bokheim-web-v4.sqlite3");
    }
    persistence = "opfs";
  } catch {
    db = new sqlite3.oo1.DB(":memory:", "c");
    const backup = await loadDatabaseBackup().catch(() => null);
    if (backup?.length) {
      const pointer = sqlite3.wasm.allocFromTypedArray(backup);
      const result = sqlite3.capi.sqlite3_deserialize(
        db.pointer!,
        "main",
        pointer,
        backup.length,
        backup.length,
        sqlite3.capi.SQLITE_DESERIALIZE_FREEONCLOSE |
          sqlite3.capi.SQLITE_DESERIALIZE_RESIZEABLE,
      );
      if (result !== sqlite3.capi.SQLITE_OK) {
        sqlite3.wasm.dealloc(pointer);
        throw new Error(`Could not restore the browser database backup (${result})`);
      }
    }
    persistence = "indexedDB" in globalThis ? "indexeddb" : "memory";
  }
  initializeWebDatabase(db);
  return { sqlite3, db, persistence };
}

export function getWebDatabase(): Promise<WebDatabase> {
  return (databasePromise ??= openDatabase());
}

/** Flushes the in-memory SQLite fallback to durable IndexedDB storage. */
let backupWriteQueue = Promise.resolve();
export async function persistWebDatabaseFallback(): Promise<void> {
  const { sqlite3, db, persistence } = await getWebDatabase();
  if (persistence !== "indexeddb") return;
  if (!db.pointer) throw new Error("The browser database is closed");
  const bytes = sqlite3.capi.sqlite3_js_db_export(db.pointer);
  backupWriteQueue = backupWriteQueue
    .catch(() => undefined)
    .then(async () => {
      if (!(await saveDatabaseBackup(bytes)))
        throw new Error("Persistent browser storage is unavailable");
    });
  await backupWriteQueue;
}

export async function getWebDatabaseHealth(): Promise<WebDatabaseHealth> {
  const { db, sqlite3, persistence } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT value FROM web_backend_metadata WHERE key = 'schema_version'",
    rowMode: 0,
    returnValue: "resultRows",
  });
  const schemaVersion = Number(rows[0]);
  if (!Number.isSafeInteger(schemaVersion))
    throw new Error("The web database schema version is invalid");
  return {
    sqlite_version: sqlite3.version.libVersion,
    schema_version: schemaVersion,
    persistence,
  };
}
