import assert from "node:assert/strict";
import test from "node:test";

import { getWebDatabase, getWebDatabaseHealth, initializeWebDatabase } from "./database.ts";

test("SQLite WASM initializes and applies the web schema", async () => {
  const health = await getWebDatabaseHealth();
  assert.match(health.sqlite_version, /^3\./);
  assert.equal(health.schema_version, 6);
  assert.equal(health.persistence, "memory");

  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT key, value FROM web_backend_metadata",
    rowMode: "object",
    returnValue: "resultRows",
  });
  assert.equal(rows.length, 1);
  assert.equal(rows[0].key, "schema_version");
  assert.equal(rows[0].value, "6");

  const bookColumns = db.exec({
    sql: "SELECT name, \"notnull\" FROM pragma_table_info('books') WHERE name IN ('storage_key', 'bytes', 'cover_bytes') ORDER BY name",
    rowMode: "array",
    returnValue: "resultRows",
  });
  assert.deepEqual(bookColumns, [["bytes", 0], ["cover_bytes", 0], ["storage_key", 0]]);
});
