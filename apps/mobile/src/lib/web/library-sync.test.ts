import assert from "node:assert/strict";
import test from "node:test";

import { getWebDatabase } from "./database.ts";
import { applyWebSyncExchange } from "./library-sync.ts";

const libraryId = "716571ff-991e-4d33-9378-ff1452b4f176";
const replicaId = "c7b1e537-e3d7-4075-a17a-b7a1b80e9cf6";
const rootId = "00000000-0000-0000-0000-000000000000";
const contentHash = "a".repeat(64);

function mutation(kind: string, entityKey: string, value: unknown, revision: number, entitySubkey = "") {
  return {
    mutation_id: `00000000-0000-4000-8000-${revision.toString().padStart(12, "0")}`,
    kind, entity_key: entityKey, entity_subkey: entitySubkey, value,
    changed_at: revision * 10, conflict_rank: 0, replica_id: replicaId, replica_seq: revision, revision,
  };
}

test("pulled EPUB state converges through shared directory projection", async () => {
  const { db } = await getWebDatabase();
  db.exec({ sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, 'Synced', ?)", bind: [libraryId, Date.now()] });
  db.exec({ sql: "INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?)", bind: [libraryId, crypto.randomUUID()] });
  const firstDirectory = "11111111-1111-4111-8111-111111111111";
  const secondDirectory = "22222222-2222-4222-8222-222222222222";
  const metadata = { title: "Shared Book", contributors: [], description: "", publication: {} };
  const mutations = [
    mutation("directory_name", firstDirectory, "Shelf", 1),
    mutation("directory_parent", firstDirectory, rootId, 2),
    mutation("directory_lifecycle", firstDirectory, "present", 3),
    mutation("directory_name", secondDirectory, "shelf", 4),
    mutation("directory_parent", secondDirectory, rootId, 5),
    mutation("directory_lifecycle", secondDirectory, "present", 6),
    mutation("book_lifecycle", contentHash, { present: { metadata, added_at: 70, format: "Epub" } }, 7),
    mutation("metadata", contentHash, metadata, 8),
    mutation("placement", contentHash, { present: true, origin_folder_id: null }, 9, firstDirectory),
    mutation("reading_position", contentHash, { location: "epubcfi(/6/4)", progress: 25 }, 10),
  ];
  await applyWebSyncExchange(libraryId, { accepted: [], rejected: [], mutations, next_cursor: 10, has_more: false });

  const directories = db.exec({ sql: "SELECT name FROM directories WHERE library_id=? ORDER BY name COLLATE NOCASE", bind: [libraryId], rowMode: 0, returnValue: "resultRows" });
  assert.deepEqual(directories, ["shelf", "shelf 2"]);
  const book = db.exec({ sql: "SELECT title, storage_key, bytes FROM books WHERE library_id=? AND content_hash=?", bind: [libraryId, contentHash], rowMode: "array", returnValue: "resultRows" });
  assert.deepEqual(book, [["Shared Book", null, null]]);
  assert.equal(db.exec({ sql: "SELECT cfi FROM reading_positions WHERE library_id=? AND content_hash=?", bind: [libraryId, contentHash], rowMode: 0, returnValue: "resultRows" })[0], "epubcfi(/6/4)");

  const deletion = mutation("book_lifecycle", contentHash, { deleted: { origin_folder_id: null } }, 11);
  deletion.conflict_rank = 1;
  await applyWebSyncExchange(libraryId, { accepted: [], rejected: [], mutations: [deletion], next_cursor: 11, has_more: false });
  assert.equal(db.exec({ sql: "SELECT count(*) FROM books WHERE library_id=?", bind: [libraryId], rowMode: 0, returnValue: "resultRows" })[0], 0);
});
