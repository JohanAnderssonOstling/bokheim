import assert from "node:assert/strict";
import test from "node:test";

import { getWebAccountStatus } from "./account-client.ts";
import { getWebDatabase } from "./database.ts";

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
