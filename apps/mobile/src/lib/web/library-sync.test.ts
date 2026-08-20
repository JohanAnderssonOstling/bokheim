import assert from "node:assert/strict";
import test from "node:test";

import { getWebDatabase } from "./database.ts";
import { applyWebSyncExchange } from "./library-sync.ts";
import {
  getOwnedWebCatalogPublications,
  getWebLibraryTrash,
} from "./library-database.ts";

const libraryId = "716571ff-991e-4d33-9378-ff1452b4f176";
const replicaId = "c7b1e537-e3d7-4075-a17a-b7a1b80e9cf6";
const rootId = "00000000-0000-0000-0000-000000000000";
const contentHash = "a".repeat(64);

function cursor(revision: number) {
  return { state_revision: revision, reading_revision: revision };
}

function mutation(
  kind: string,
  entityKey: string,
  value: unknown,
  revision: number,
  entitySubkey = "",
) {
  return {
    mutation_id: `00000000-0000-4000-8000-${revision.toString().padStart(12, "0")}`,
    kind,
    entity_key: entityKey,
    entity_subkey: entitySubkey,
    value,
    changed_at: revision * 10,
    conflict_rank: 0,
    replica_id: replicaId,
    replica_seq: revision,
    revision,
  };
}

test("pulled EPUB state converges through shared directory projection", async () => {
  const { db } = await getWebDatabase();
  db.exec({
    sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, 'Synced', ?)",
    bind: [libraryId, Date.now()],
  });
  db.exec({
    sql: "INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?)",
    bind: [libraryId, crypto.randomUUID()],
  });
  const firstDirectory = "11111111-1111-4111-8111-111111111111";
  const secondDirectory = "22222222-2222-4222-8222-222222222222";
  const metadata = {
    title: "Shared Book",
    contributors: [],
    description: "",
    publication: {},
  };
  const mutations = [
    mutation("directory_name", firstDirectory, "Shelf", 1),
    mutation("directory_parent", firstDirectory, rootId, 2),
    mutation("directory_lifecycle", firstDirectory, "present", 3),
    mutation("directory_name", secondDirectory, "shelf", 4),
    mutation("directory_parent", secondDirectory, rootId, 5),
    mutation("directory_lifecycle", secondDirectory, "present", 6),
    mutation(
      "book_lifecycle",
      contentHash,
      { present: { metadata, added_at: 70, format: "Epub" } },
      7,
    ),
    mutation("metadata", contentHash, metadata, 8),
    mutation(
      "placement",
      contentHash,
      { present: true, origin_folder_id: null },
      9,
      firstDirectory,
    ),
    mutation(
      "reading_position",
      contentHash,
      { location: "epubcfi(/6/4)", progress: 25 },
      10,
    ),
  ];
  await applyWebSyncExchange(libraryId, {
    accepted: [],
    rejected: [],
    mutations,
    next_cursor: cursor(10),
    has_more: false,
  });

  const directories = db.exec({
    sql: "SELECT name FROM directories WHERE library_id=? ORDER BY name COLLATE NOCASE",
    bind: [libraryId],
    rowMode: 0,
    returnValue: "resultRows",
  });
  assert.deepEqual(directories, ["shelf", "shelf 2"]);
  const book = db.exec({
    sql: "SELECT title, storage_key, bytes FROM books WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
    rowMode: "array",
    returnValue: "resultRows",
  });
  assert.deepEqual(book, [["Shared Book", null, null]]);
  assert.equal(
    db.exec({
      sql: "SELECT cfi FROM reading_positions WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
    "epubcfi(/6/4)",
  );

  const deletion = mutation(
    "book_lifecycle",
    contentHash,
    { deleted: { origin_folder_id: null } },
    11,
  );
  deletion.conflict_rank = 1;
  await applyWebSyncExchange(libraryId, {
    accepted: [],
    rejected: [],
    mutations: [deletion],
    next_cursor: cursor(11),
    has_more: false,
  });
  assert.deepEqual(
    (await getWebLibraryTrash(libraryId)).books.map(({ title }) => title),
    ["Shared Book"],
  );

  const restored = mutation(
    "book_lifecycle",
    contentHash,
    { present: { metadata, added_at: 70, format: "Epub" } },
    12,
  );
  await applyWebSyncExchange(libraryId, {
    accepted: [],
    rejected: [],
    mutations: [restored],
    next_cursor: cursor(12),
    has_more: false,
  });
  assert.deepEqual((await getWebLibraryTrash(libraryId)).books, []);
  assert.equal(
    db.exec({
      sql: "SELECT count(*) FROM books WHERE library_id=?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
    1,
  );
});

test("pulled catalogue ownership is projected after its book", async () => {
  const syncedLibraryId = "826571ff-991e-4d33-9378-ff1452b4f176";
  const syncedContentHash = "b".repeat(64);
  const { db } = await getWebDatabase();
  db.exec({
    sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, 'Catalogue sync', ?)",
    bind: [syncedLibraryId, Date.now()],
  });
  db.exec({
    sql: "INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?)",
    bind: [syncedLibraryId, crypto.randomUUID()],
  });

  const metadata = {
    title: "Shared catalogue book",
    contributors: [],
    description: "",
    publication: {},
  };
  await applyWebSyncExchange(syncedLibraryId, {
    accepted: [],
    rejected: [],
    next_cursor: cursor(2),
    has_more: false,
    mutations: [
      mutation(
        "book_lifecycle",
        syncedContentHash,
        { present: { metadata, added_at: 10, format: "Epub" } },
        1,
      ),
      mutation(
        "catalog_publication",
        "litteraturbanken",
        {
          publication: { edition_id: "lb-edition-1", work_id: "lb-work-1" },
          content_hash: syncedContentHash,
        },
        2,
        "lb-edition-1",
      ),
    ],
  });

  assert.deepEqual(await getOwnedWebCatalogPublications(syncedLibraryId), [
    {
      source_id: "litteraturbanken",
      publication: { edition_id: "lb-edition-1", work_id: "lb-work-1" },
    },
  ]);
  assert.equal(
    db.exec({
      sql: "SELECT content_hash FROM web_catalog_publications WHERE library_id=? AND source_id=? AND edition_id=?",
      bind: [syncedLibraryId, "litteraturbanken", "lb-edition-1"],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
    syncedContentHash,
  );
});

test("pulled PDF state keeps a PDF filename", async () => {
  const pdfLibraryId = "926571ff-991e-4d33-9378-ff1452b4f176";
  const pdfContentHash = "c".repeat(64);
  const { db } = await getWebDatabase();
  db.exec({
    sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, 'PDF sync', ?)",
    bind: [pdfLibraryId, Date.now()],
  });
  db.exec({
    sql: "INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?)",
    bind: [pdfLibraryId, crypto.randomUUID()],
  });
  const metadata = {
    title: "Shared PDF",
    contributors: [],
    description: "",
    publication: {},
  };
  await applyWebSyncExchange(pdfLibraryId, {
    accepted: [],
    rejected: [],
    next_cursor: cursor(1),
    has_more: false,
    mutations: [
      mutation(
        "book_lifecycle",
        pdfContentHash,
        { present: { metadata, added_at: 10, format: "Pdf" } },
        1,
      ),
    ],
  });
  assert.equal(
    db.exec({
      sql: "SELECT file_name FROM books WHERE library_id=? AND content_hash=?",
      bind: [pdfLibraryId, pdfContentHash],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
    "Shared PDF.pdf",
  );
});
