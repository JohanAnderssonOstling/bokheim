import assert from "node:assert/strict";
import test from "node:test";

import { synchronizeWebLibraryThumbnails } from "./blob-sync.ts";
import { getWebDatabase } from "./database.ts";

test("thumbnail sync uploads local covers and persists remote covers without echoing them", async () => {
  const { db } = await getWebDatabase();
  const libraryId = crypto.randomUUID();
  const uploadHash = "b".repeat(64);
  const downloadHash = "c".repeat(64);
  const missingHash = "d".repeat(64);
  const uploadedCover = new Uint8Array([0xff, 0xd8, 1, 2, 0xff, 0xd9]);
  const downloadedCover = new Uint8Array([0xff, 0xd8, 3, 4, 0xff, 0xd9]);
  db.exec({ sql: "INSERT INTO libraries (library_id,library_name,created_at) VALUES (?,?,?)", bind: [libraryId, "Thumbnail test", Date.now()] });
  db.exec({
    sql: "INSERT OR REPLACE INTO web_account_profile (singleton,server_url,user_id,email) VALUES (1,?,?,?)",
    bind: ["https://api.bokheim.se", "thumbnail-reader", "thumbnail@example.com"],
  });
  for (const [hash, cover] of [[uploadHash, uploadedCover], [downloadHash, null], [missingHash, null]] as const) db.exec({
    sql: "INSERT INTO books (library_id,content_hash,file_name,title,cover_bytes,added_at) VALUES (?,?,?,?,?,?)",
    bind: [libraryId, hash, `${hash}.epub`, hash, cover, Date.now()],
  });

  const requests: Array<{ url: string; method: string; body: Uint8Array | null }> = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init = {}) => {
    const url = String(input);
    const method = init.method ?? "GET";
    const body = init.body instanceof Uint8Array ? init.body.slice() : null;
    requests.push({ url, method, body });
    if (url.endsWith(`/${downloadHash}`)) return new Response(downloadedCover, { status: 200, headers: { "Content-Type": "image/jpeg" } });
    if (url.endsWith(`/${missingHash}`)) return new Response(null, { status: 404 });
    assert.equal(method, "PUT");
    assert.deepEqual(body, uploadedCover);
    return new Response(null, { status: 200 });
  };
  try {
    await synchronizeWebLibraryThumbnails(libraryId);
    await synchronizeWebLibraryThumbnails(libraryId);
  } finally {
    globalThis.fetch = originalFetch;
  }

  assert.equal(requests.filter(({ url, method }) => url.endsWith(`/${uploadHash}`) && method === "PUT").length, 1);
  assert.equal(requests.filter(({ url, method }) => url.endsWith(`/${downloadHash}`) && method === "GET").length, 1);
  assert.equal(requests.filter(({ url, method }) => url.endsWith(`/${downloadHash}`) && method === "PUT").length, 0);
  assert.equal(requests.filter(({ url }) => url.endsWith(`/${missingHash}`)).length, 2);
  const stored = db.exec({ sql: "SELECT cover_bytes FROM books WHERE library_id=? AND content_hash=?", bind: [libraryId, downloadHash], rowMode: 0, returnValue: "resultRows" })[0];
  assert.deepEqual(stored, downloadedCover);
  assert.deepEqual(db.exec({
    sql: "SELECT direction,state FROM web_transfers WHERE library_id=? AND kind='thumbnail' ORDER BY direction",
    bind: [libraryId], rowMode: "array", returnValue: "resultRows",
  }), [["download", "completed"], ["upload", "completed"]]);
  db.exec({ sql: "DELETE FROM libraries WHERE library_id=?", bind: [libraryId] });
});
