import assert from "node:assert/strict";
import test from "node:test";

import { getWebDatabase, getWebDatabaseHealth, initializeWebDatabase } from "./database.ts";

test("SQLite WASM initializes and applies the web schema", async () => {
  const health = await getWebDatabaseHealth();
  assert.match(health.sqlite_version, /^3\./);
  assert.equal(health.schema_version, 15);
  assert.equal(health.persistence, "memory");

  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT key, value FROM web_backend_metadata",
    rowMode: "object",
    returnValue: "resultRows",
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].key, "schema_version");
  assert.equal(rows[0].value, "15");

  const bookColumns = db.exec({
    sql: "SELECT name, \"notnull\" FROM pragma_table_info('books') WHERE name IN ('storage_key', 'bytes', 'cover_bytes', 'description') ORDER BY name",
    rowMode: "array",
    returnValue: "resultRows",
  });
  assert.deepEqual(bookColumns, [["bytes", 0], ["cover_bytes", 0], ["description", 1], ["storage_key", 0]]);

  const trashTables = db.exec({
    sql: "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('web_trash_books', 'web_trash_folders') ORDER BY name",
    rowMode: 0,
    returnValue: "resultRows",
  });
  assert.deepEqual(trashTables, ["web_trash_books", "web_trash_folders"]);

  const catalogTables = db.exec({
    sql: "SELECT name FROM sqlite_master WHERE type='table' AND name='web_catalog_publications'",
    rowMode: 0,
    returnValue: "resultRows",
  });
  assert.deepEqual(catalogTables, ["web_catalog_publications"]);

  const taxonomyTables = db.exec({
    sql: "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('web_taxonomy_concepts','web_taxonomy_resolutions') ORDER BY name",
    rowMode: 0,
    returnValue: "resultRows",
  });
  assert.deepEqual(taxonomyTables, ["web_taxonomy_concepts", "web_taxonomy_resolutions"]);

  const ownershipTables = db.exec({
    sql: "SELECT name FROM sqlite_master WHERE type='table' AND name='web_library_owners'",
    rowMode: 0,
    returnValue: "resultRows",
  });
  assert.deepEqual(ownershipTables, ["web_library_owners"]);

  const richMetadataTables = db.exec({
    sql: `SELECT name FROM sqlite_master WHERE type='table'
          AND name IN ('web_external_entities','web_external_place_edges','web_book_external_entities') ORDER BY name`,
    rowMode: 0,
    returnValue: "resultRows",
  });
  assert.deepEqual(richMetadataTables, ["web_book_external_entities", "web_external_entities", "web_external_place_edges"]);
});

test("schema migration adds account ownership without retaining unscoped deletions", async () => {
  const { sqlite3 } = await getWebDatabase();
  const legacy = new sqlite3.oo1.DB(":memory:", "c");
  try {
    legacy.exec(`
      CREATE TABLE web_library_tombstones (
        library_id TEXT PRIMARY KEY NOT NULL,
        deleted_at INTEGER NOT NULL
      ) STRICT;
      INSERT INTO web_library_tombstones (library_id,deleted_at) VALUES ('legacy-library',1);
    `);
    initializeWebDatabase(legacy);
    assert.deepEqual(legacy.exec({
      sql: "SELECT name FROM pragma_table_info('web_library_tombstones') ORDER BY cid",
      rowMode: 0, returnValue: "resultRows",
    }), ["library_id", "deleted_at", "account_id"]);
    assert.equal(legacy.exec({
      sql: "SELECT count(*) FROM web_library_tombstones",
      rowMode: 0, returnValue: "resultRows",
    })[0], 0);
  } finally {
    legacy.close();
  }
});

test("schema migration promotes the scalar sync cursor to both channels", async () => {
  const { sqlite3 } = await getWebDatabase();
  const legacy = new sqlite3.oo1.DB(":memory:", "c");
  try {
    legacy.exec(`
      CREATE TABLE libraries (
        library_id TEXT PRIMARY KEY NOT NULL,
        library_name TEXT NOT NULL CHECK (length(trim(library_name)) BETWEEN 1 AND 255),
        created_at INTEGER NOT NULL
      ) STRICT;
      INSERT INTO libraries (library_id,library_name,created_at) VALUES ('legacy-library','Legacy',1);
      CREATE TABLE web_sync_state (
        library_id TEXT PRIMARY KEY NOT NULL REFERENCES libraries(library_id) ON DELETE CASCADE,
        replica_id TEXT NOT NULL,
        last_replica_seq INTEGER NOT NULL DEFAULT 0 CHECK (last_replica_seq >= 0),
        last_server_revision INTEGER CHECK (last_server_revision IS NULL OR last_server_revision > 0),
        last_synced_at INTEGER
      ) STRICT;
      INSERT INTO web_sync_state (library_id,replica_id,last_server_revision)
      VALUES ('legacy-library','legacy-replica',42);
    `);
    initializeWebDatabase(legacy);
    assert.deepEqual(legacy.exec({
      sql: "SELECT last_state_revision,last_reading_revision FROM web_sync_state WHERE library_id='legacy-library'",
      rowMode: "array", returnValue: "resultRows",
    }), [[42, 42]]);
  } finally {
    legacy.close();
  }
});
