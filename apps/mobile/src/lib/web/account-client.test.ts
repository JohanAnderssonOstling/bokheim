import assert from "node:assert/strict";
import test from "node:test";

import { getWebAccountStatus } from "./account-client.ts";
import { getWebDatabase } from "./database.ts";
import { createWebLibrary, getWebLibraryHome, listWebLibraries } from "./library-database.ts";

test("browser login persistence stores profile metadata without bearer tokens", async () => {
  const { db } = await getWebDatabase();
  db.exec({
    sql: "INSERT INTO web_account_profile (singleton, server_url, user_id, email) VALUES (1, ?, ?, ?)",
    bind: ["https://api.bokheim.se", "reader-id", "reader@example.com"],
  });

  assert.deepEqual(await getWebAccountStatus(), {
    signed_in: true,
    server_url: "https://api.bokheim.se",
    email: "reader@example.com",
  });
  assert.deepEqual(db.exec({ sql: "SELECT name FROM sqlite_schema WHERE type='table' AND name='web_account_session'", rowMode: 0, returnValue: "resultRows" }), []);
});

test("browser libraries remain isolated when a different account signs in", async () => {
  const { db } = await getWebDatabase();
  const firstUser = `first-${crypto.randomUUID()}`;
  const secondUser = `second-${crypto.randomUUID()}`;
  db.exec({
    sql: "INSERT INTO web_account_profile (singleton,server_url,user_id,email) VALUES (1,?,?,?) ON CONFLICT (singleton) DO UPDATE SET user_id=excluded.user_id,email=excluded.email",
    bind: ["https://api.bokheim.se", firstUser, "first@example.com"],
  });
  const firstLibrary = await createWebLibrary(`Private ${crypto.randomUUID()}`);

  db.exec({
    sql: "UPDATE web_account_profile SET user_id=?,email=? WHERE singleton=1",
    bind: [secondUser, "second@example.com"],
  });
  assert.equal((await listWebLibraries()).some(({ library_id }) => library_id === firstLibrary.library_id), false);
  await assert.rejects(getWebLibraryHome(firstLibrary.library_id), /does not exist/);

  db.exec({
    sql: "UPDATE web_account_profile SET user_id=?,email=? WHERE singleton=1",
    bind: [firstUser, "first@example.com"],
  });
  assert.equal((await listWebLibraries()).some(({ library_id }) => library_id === firstLibrary.library_id), true);

  db.exec({ sql: "DELETE FROM libraries WHERE library_id=?", bind: [firstLibrary.library_id] });
});
