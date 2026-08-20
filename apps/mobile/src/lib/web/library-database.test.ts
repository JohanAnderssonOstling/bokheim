import assert from "node:assert/strict";
import test from "node:test";
import { ROOT_DIRECTORY_ID } from "../library/directory-id.ts";
import { storedZip, testEpub } from "./test-epub.ts";

import {
  attachRemoteWebLibrary,
  createWebLibrary,
  createWebDirectory,
  copyWebBookToFolder,
  enrichWebLibraryAuthors,
  enrichWebLibraryRich,
  getWebFolderDestinations,
  getWebDirectoryPath,
  getWebLibraryAuthors,
  getWebLibraryFolder,
  getWebLibrarySubjects,
  getWebLibraryBookCount,
  getWebPendingSyncMutationCount,
  getWebBookFileName,
  getWebBookThumbnail,
  getWebLibraryHome,
  getWebLibraryTrash,
  getOwnedWebCatalogPublications,
  getWebReadingPosition,
  importWebBook,
  listWebLibraries,
  moveWebBookToFolder,
  moveWebFolder,
  moveWebBookToTrash,
  moveWebFolderToTrash,
  openWebBook,
  purgeWebTrashedFolder,
  recordWebCatalogPublication,
  removeWebBookFromFolder,
  removeWebLibrary,
  renameWebFolder,
  restoreWebTrashedBook,
  restoreWebTrashedFolder,
  searchWebLibrary,
  searchWebLibrarySubject,
  updateWebReadingPosition,
} from "./library-database.ts";

test("browser folder editing matches native copy, move, rename and remove behavior", async () => {
  const library = await createWebLibrary(`Folder editing ${crypto.randomUUID()}`);
  const first = await createWebDirectory(library.library_id, ROOT_DIRECTORY_ID, "First");
  const second = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "Second",
  );
  const nested = await createWebDirectory(library.library_id, first.id, "Nested");

  assert.deepEqual(
    (await getWebFolderDestinations(library.library_id)).map(({ path }) => path),
    ["Library", "First", "First / Nested", "Second"],
  );
  assert.equal(await renameWebFolder(library.library_id, second.id, "First"), "First 2");
  assert.equal(
    await renameWebFolder(library.library_id, nested.id, "Renamed"),
    "Renamed",
  );
  assert.equal(await moveWebFolder(library.library_id, nested.id, second.id), "Renamed");
  await assert.rejects(
    moveWebFolder(library.library_id, second.id, nested.id),
    /descendants/,
  );

  const file = new File([testEpub("Managed Book")], "Managed Book.epub", {
    type: "application/epub+zip",
  });
  const contentHash = await importWebBook(
    { library_id: library.library_id, directory_id: first.id },
    file,
    file.name,
  );
  assert.equal(
    await copyWebBookToFolder(library.library_id, contentHash, first.id, second.id),
    file.name,
  );
  assert.equal((await getWebLibraryFolder(library.library_id, first.id)).books.length, 1);
  assert.equal(
    (await getWebLibraryFolder(library.library_id, second.id)).books.length,
    1,
  );

  assert.equal(
    await moveWebBookToFolder(library.library_id, contentHash, second.id, nested.id),
    file.name,
  );
  assert.equal(
    (await getWebLibraryFolder(library.library_id, second.id)).books.length,
    0,
  );
  assert.equal(
    (await getWebLibraryFolder(library.library_id, nested.id)).books.length,
    1,
  );
  assert.equal(
    await removeWebBookFromFolder(library.library_id, contentHash, nested.id),
    false,
  );
  assert.equal(
    await removeWebBookFromFolder(library.library_id, contentHash, first.id),
    true,
  );
  assert.deepEqual(
    (await getWebLibraryTrash(library.library_id)).books.map(({ title }) => title),
    ["Managed Book"],
  );

  await removeWebLibrary(library.library_id);
});

test("browser ISBN enrichment replaces a provisional same-name author before upload", async () => {
  const library = await createWebLibrary(`Author enrichment ${crypto.randomUUID()}`);
  const bytes = storedZip({
    mimetype: "application/epub+zip",
    "META-INF/container.xml":
      '<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    "OPS/book.opf":
      "<package><metadata><dc:title>Identified</dc:title><dc:creator>Alex Smith</dc:creator><dc:identifier>9780306406157</dc:identifier></metadata><manifest></manifest></package>",
  });
  const file = new File([bytes], "Identified.epub", { type: "application/epub+zip" });
  await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    file,
    file.name,
  );
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () =>
    new Response(
      JSON.stringify({
        results: [
          {
            requested_isbn: "9780306406157",
            canonical_isbn13: "9780306406157",
            status: "matched",
            matches: [
              { authors: [{ open_library_author_id: "OL100A", name: "Alex Smith" }] },
            ],
          },
        ],
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
  try {
    assert.equal(await enrichWebLibraryAuthors(library.library_id), 1);
  } finally {
    globalThis.fetch = originalFetch;
  }

  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  const external = db.exec({
    sql: "SELECT external_id,author_id FROM web_author_external_identifiers WHERE library_id=?",
    bind: [library.library_id],
    rowMode: "array",
    returnValue: "resultRows",
  }) as unknown[][];
  assert.equal(external[0][0], "OL100A");
  assert.equal(
    external[0][1],
    "445826e1-4030-5774-86b1-ae8d4e606d3d",
    "web and native clients use the same authority-derived UUID",
  );
  assert.equal(
    (await getWebLibraryAuthors(library.library_id, null)).authors[0].id,
    external[0][1],
  );
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 4);
  assert.equal(
    await enrichWebLibraryAuthors(library.library_id),
    0,
    "a completed snapshot lookup is not repeated on every sync",
  );
  await removeWebLibrary(library.library_id);
});

test("browser rich enrichment presents directly referenced places and people as flat grids", async () => {
  const library = await createWebLibrary(`Rich enrichment ${crypto.randomUUID()}`);
  const bytes = storedZip({
    mimetype: "application/epub+zip",
    "META-INF/container.xml":
      '<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    "OPS/book.opf":
      "<package><metadata><dc:title>Paris Book</dc:title><dc:identifier>9780306406157</dc:identifier></metadata><manifest></manifest></package>",
  });
  const file = new File([bytes], "Paris Book.epub", { type: "application/epub+zip" });
  await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    file,
    file.name,
  );
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    if (String(input).includes("taxonomy.bokheim.se")) {
      if (init?.method === "HEAD")
        return new Response(null, { headers: { ETag: '"test-taxonomy-release"' } });
      const request = JSON.parse(String(init?.body)) as {
        codes: Array<{ system: string; code: string }>;
      };
      const literatureLeaf = "bokheim:lcc-detail:04b519c3-d098-5aac-8169-0c9d4c05af87";
      const analysisLeaf = "unified:0864b3fe-19d6-575e-b85b-a1d4491da38d";
      return new Response(
        JSON.stringify({
          release_id: "test-taxonomy-release",
          results: request.codes.map((code) => ({
            ...code,
            matched_concept_ids: [code.system === "lcc" ? literatureLeaf : analysisLeaf],
          })),
          concepts: [
            {
              concept_id: "bokheim:subject:humanities",
              preferred_label: "Humanities",
              parents: [],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: "unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854",
              preferred_label: "Literature",
              parents: [{ concept_id: "bokheim:subject:humanities", display_order: 0 }],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: "bokheim:subject:literature:romance",
              preferred_label: "Romance-Language Literatures",
              parents: [
                {
                  concept_id: "unified:6aaa3a4d-bf1d-5bb0-8cda-617aff1c5854",
                  display_order: 0,
                },
              ],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: "unified:60d4653a-8bc0-596f-b83e-cdecf0651020",
              preferred_label: "French & Related Literatures",
              parents: [
                { concept_id: "bokheim:subject:literature:romance", display_order: 0 },
              ],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: "unified:1866b1cc-6191-503a-9acf-ee7fd97e070d",
              preferred_label: "Modern literature",
              parents: [
                {
                  concept_id: "unified:60d4653a-8bc0-596f-b83e-cdecf0651020",
                  display_order: 0,
                },
              ],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: literatureLeaf,
              preferred_label: "1900-1960",
              parents: [
                {
                  concept_id: "unified:1866b1cc-6191-503a-9acf-ee7fd97e070d",
                  display_order: 0,
                },
              ],
              source_selectors: { lcc: ["PQ2600..PQ2651@4"] },
              source_labels: [],
            },
            {
              concept_id: "unified:2d01b451-4e64-5f6b-a068-a6f7ea63e386",
              preferred_label: "Science & Nature",
              parents: [],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: "unified:f158bcdc-8f35-5cea-baa7-fce8bcbdc3b4",
              preferred_label: "Mathematics",
              parents: [
                {
                  concept_id: "unified:2d01b451-4e64-5f6b-a068-a6f7ea63e386",
                  display_order: 0,
                },
              ],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: "bokheim:mathematics:analysis-calculus",
              preferred_label: "Analysis, Calculus & Differential Equations",
              parents: [
                {
                  concept_id: "unified:f158bcdc-8f35-5cea-baa7-fce8bcbdc3b4",
                  display_order: 0,
                },
              ],
              source_selectors: {},
              source_labels: [],
            },
            {
              concept_id: analysisLeaf,
              preferred_label: "Analysis",
              parents: [
                { concept_id: "bokheim:mathematics:analysis-calculus", display_order: 0 },
              ],
              source_selectors: { ddc: ["515"] },
              source_labels: [],
            },
          ],
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      );
    }
    return new Response(
      JSON.stringify({
        results: [
          {
            requested_isbn: "9780306406157",
            canonical_isbn13: "9780306406157",
            status: "matched",
            matches: [
              {
                classifications: [
                  {
                    scheme: "library_of_congress",
                    notation: "PQ2631.R63",
                    source: "work",
                  },
                  { scheme: "dewey_decimal", notation: "515.33", source: "work" },
                ],
                places: [
                  { wikidata_id: "Q90", name: "Paris", source: "work" },
                  { wikidata_id: null, name: "Atlantis", source: "work" },
                ],
                people: [{ wikidata_id: "Q7259", name: "Ada Lovelace", source: "work" }],
              },
            ],
          },
        ],
        places: [
          {
            wikidata_id: "Q90",
            label: "Paris",
            aliases: [],
            parent_ids: [],
            country_ids: ["Q142"],
            latitude_e7: null,
            longitude_e7: null,
          },
          {
            wikidata_id: "Q142",
            label: "France",
            aliases: [],
            parent_ids: ["Q46"],
            country_ids: [],
            latitude_e7: null,
            longitude_e7: null,
          },
          {
            wikidata_id: "Q46",
            label: "Europe",
            aliases: [],
            parent_ids: [],
            country_ids: [],
            latitude_e7: null,
            longitude_e7: null,
          },
        ],
        people: [
          {
            wikidata_id: "Q7259",
            label: "Ada Lovelace",
            aliases: [],
            birth_year: 1815,
            death_year: 1852,
            fictional: false,
          },
        ],
      }),
      { status: 200, headers: { "Content-Type": "application/json" } },
    );
  };
  try {
    assert.equal(await enrichWebLibraryRich(library.library_id), 1);
  } finally {
    globalThis.fetch = originalFetch;
  }

  const roots = await getWebLibrarySubjects(library.library_id, null);
  assert.equal(
    roots.children.some(({ id }) => id === "__places__"),
    true,
  );
  assert.equal(
    roots.children.some(({ id }) => id === "__people__"),
    true,
  );
  assert.equal(roots.children.find(({ id }) => id === "__people__")?.name, "Persons");
  assert.equal(
    roots.children.some(({ id }) => id.startsWith("LCC")),
    false,
  );
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, "__places__")).children.map(
      ({ name }) => name,
    ),
    ["Atlantis", "Paris"],
  );
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, "__people__")).children.map(
      ({ name }) => name,
    ),
    ["Ada Lovelace"],
  );
  assert.equal(
    (await getWebLibrarySubjects(library.library_id, "__places__/Q90")).books.length,
    1,
  );
  assert.equal(
    (await getWebLibrarySubjects(library.library_id, "__places__/OLP-41746c616e746973"))
      .books.length,
    1,
  );
  assert.equal(
    (await getWebLibrarySubjects(library.library_id, "__people__/Q7259")).books.length,
    1,
  );
  assert.equal(
    (await searchWebLibrarySubject(library.library_id, "__places__/Q90", "Paris", 10))
      .title_matches.length,
    1,
  );
  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  const synchronized = JSON.parse(
    String(
      db.exec({
        sql: "SELECT value_json FROM web_sync_cells WHERE library_id=? AND kind='metadata'",
        bind: [library.library_id],
        rowMode: 0,
        returnValue: "resultRows",
      })[0],
    ),
  ) as { publication: { subject_paths: string[] } };
  assert.equal(
    synchronized.publication.subject_paths.includes(
      "Humanities / Literature / Romance-Language Literatures / French & Related Literatures / Modern literature / 1900-1960",
    ),
    true,
  );
  assert.equal(
    synchronized.publication.subject_paths.includes(
      "Science & Nature / Mathematics / Analysis, Calculus & Differential Equations / Analysis",
    ),
    true,
  );
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, "Science & Nature")).books.map(
      ({ title }) => title,
    ),
    ["Paris Book"],
  );
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, "Humanities")).books.map(
      ({ title }) => title,
    ),
    ["Paris Book"],
  );
  await removeWebLibrary(library.library_id);
});

test("remote catalogue entries attach idempotently and update their name", async () => {
  const libraryId = crypto.randomUUID();
  await attachRemoteWebLibrary(libraryId, "Remote Library");
  await attachRemoteWebLibrary(libraryId, "Renamed Remotely");
  const matches = (await listWebLibraries()).filter(
    ({ library_id }) => library_id === libraryId,
  );
  assert.deepEqual(
    matches.map(({ library_name }) => library_name),
    ["Renamed Remotely"],
  );
  assert.equal(await getWebPendingSyncMutationCount(libraryId), 0);
  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  assert.equal(
    db.exec({
      sql: "SELECT count(*) FROM web_remote_libraries WHERE library_id=?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
    1,
  );
  await removeWebLibrary(libraryId);
});

test("browser Trash hides and restores books and folders", async () => {
  const library = await createWebLibrary(`Trash test ${crypto.randomUUID()}`);
  const folder = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "Discarded",
  );
  const bytes = testEpub("Temporary");
  const file = new File([bytes], "Temporary.epub", { type: "application/epub+zip" });
  await importWebBook(
    { library_id: library.library_id, directory_id: folder.id },
    file,
    file.name,
  );
  const book = (await getWebLibraryHome(library.library_id)).recently_added[0];

  await moveWebFolderToTrash(library.library_id, folder.id);
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 9);
  assert.deepEqual((await getWebLibraryHome(library.library_id)).recently_added, []);
  assert.deepEqual(
    (await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID)).children,
    [],
  );
  await assert.rejects(
    getWebLibraryFolder(library.library_id, folder.id),
    /does not exist/,
  );
  const trash = await getWebLibraryTrash(library.library_id);
  assert.deepEqual(
    trash.books.map(({ title }) => title),
    ["Temporary"],
  );
  assert.deepEqual(
    trash.folders.map(({ name }) => name),
    ["Discarded"],
  );

  await restoreWebTrashedFolder(library.library_id, folder.id);
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 12);
  assert.equal((await getWebLibraryHome(library.library_id)).recently_added.length, 1);
  assert.equal(
    (await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID)).children.length,
    1,
  );
  assert.equal(
    (await getWebLibraryFolder(library.library_id, folder.id)).books[0].content_hash,
    book.content_hash,
  );
  assert.deepEqual(await getWebLibraryTrash(library.library_id), {
    books: [],
    folders: [],
  });
  await removeWebLibrary(library.library_id);
});

test("direct book Trash updates placements and permanent folder deletion purges its subtree", async () => {
  const library = await createWebLibrary(`Purge test ${crypto.randomUUID()}`);
  const parent = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "Parent",
  );
  const child = await createWebDirectory(library.library_id, parent.id, "Child");
  const file = new File([testEpub("Placed Book")], "Placed Book.epub", {
    type: "application/epub+zip",
  });
  await importWebBook(
    { library_id: library.library_id, directory_id: child.id },
    file,
    file.name,
  );
  const book = (await getWebLibraryHome(library.library_id)).recently_added[0];

  await moveWebBookToTrash(library.library_id, book.content_hash);
  assert.equal(
    (await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID)).children[0]
      .book_count,
    0,
  );
  await restoreWebTrashedBook(library.library_id, book.content_hash);
  assert.equal(
    (await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID)).children[0]
      .book_count,
    1,
  );

  await moveWebFolderToTrash(library.library_id, parent.id);
  await purgeWebTrashedFolder(library.library_id, parent.id);
  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  const lifecycleRows = db.exec({
    sql: "SELECT entity_key,value_json FROM web_sync_cells WHERE library_id=? AND kind='directory_lifecycle' ORDER BY entity_key",
    bind: [library.library_id],
    rowMode: "array",
    returnValue: "resultRows",
  });
  assert.deepEqual(
    lifecycleRows,
    [
      [parent.id, '\"purged\"'],
      [child.id, '\"purged\"'],
    ].sort(),
  );
  assert.deepEqual(
    (await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID)).children,
    [],
  );
  await removeWebLibrary(library.library_id);
});

test("browser libraries can be created, listed, opened and removed", async () => {
  const initial = await listWebLibraries();
  assert.ok(initial.length > 0);
  const name = `Browser library ${crypto.randomUUID()}`;
  const created = await createWebLibrary(name);
  assert.equal(created.library_name, name);
  assert.match(created.library_path, /^opfs:\/\/bokheim\//);
  assert.ok(
    (await listWebLibraries()).some(
      ({ library_id }) => library_id === created.library_id,
    ),
  );
  assert.deepEqual(await getWebLibraryHome(created.library_id), {
    recently_read: [],
    recently_added: [],
  });
  assert.equal(await getWebLibraryBookCount(created.library_id), 0);
  await removeWebLibrary(created.library_id);
  assert.ok(
    !(await listWebLibraries()).some(
      ({ library_id }) => library_id === created.library_id,
    ),
  );
});

test("the core browser reader loop imports, reopens and tracks a book", async () => {
  const library = await createWebLibrary(`Reader test ${crypto.randomUUID()}`);
  const bytes = testEpub("Example Book");
  const file = new File([bytes], "Example Book.epub", { type: "application/epub+zip" });

  await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    file,
    file.name,
  );
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 3);
  const home = await getWebLibraryHome(library.library_id);
  assert.equal(await getWebLibraryBookCount(library.library_id), 1);
  assert.equal(home.recently_added.length, 1);
  const book = home.recently_added[0];
  assert.equal(book.title, "Example Book");
  assert.equal(book.downloaded, true);
  assert.equal(
    await getWebBookFileName(library.library_id, book.content_hash),
    file.name,
  );

  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  const storage = db.exec({
    sql: "SELECT storage_key, length(bytes) FROM books WHERE library_id = ? AND content_hash = ?",
    bind: [library.library_id, book.content_hash],
    rowMode: "array",
    returnValue: "resultRows",
  });
  assert.deepEqual(storage, [[null, bytes.length]]);
  assert.deepEqual(
    await getWebBookThumbnail(library.library_id, book.content_hash),
    new Uint8Array(),
  );
  const cover = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);
  db.exec({
    sql: "UPDATE books SET cover_bytes = ? WHERE library_id = ? AND content_hash = ?",
    bind: [cover, library.library_id, book.content_hash],
  });
  assert.deepEqual(
    await getWebBookThumbnail(library.library_id, book.content_hash),
    cover,
  );

  const reopenedSource = await openWebBook(library.library_id, book.content_hash);
  assert.ok(reopenedSource instanceof Blob);
  const reopened = new Uint8Array(await reopenedSource.arrayBuffer());
  assert.deepEqual(reopened, bytes);
  assert.equal(await getWebReadingPosition(library.library_id, book.content_hash), null);

  await updateWebReadingPosition(
    library.library_id,
    book.content_hash,
    "epubcfi(/6/4)",
    0.25,
  );
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 4);
  assert.equal(
    await getWebReadingPosition(library.library_id, book.content_hash),
    "epubcfi(/6/4)",
  );
  assert.equal(
    (await getWebLibraryHome(library.library_id)).recently_read[0].progress,
    0.25,
  );

  await removeWebLibrary(library.library_id);
});

test("invalid PDF files are rejected before storage", async () => {
  const library = await createWebLibrary(`Invalid import ${crypto.randomUUID()}`);
  const invalid = new File([new Uint8Array([1, 2, 3])], "broken.pdf", {
    type: "application/pdf",
  });
  await assert.rejects(
    importWebBook(
      { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
      invalid,
      invalid.name,
    ),
    /not a valid PDF/,
  );
  assert.deepEqual((await getWebLibraryHome(library.library_id)).recently_added, []);
  await removeWebLibrary(library.library_id);
});

test("PDF files retain their format when stored and reopened", async () => {
  const library = await createWebLibrary(`PDF import ${crypto.randomUUID()}`);
  const bytes = new TextEncoder().encode("%PDF-1.7\n1 0 obj\n<<>>\nendobj\n%%EOF");
  const file = new File([bytes], "Reference.pdf", { type: "application/pdf" });
  const cover = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);
  const contentHash = await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    file,
    file.name,
    undefined,
    cover,
  );

  assert.equal(
    await getWebBookFileName(library.library_id, contentHash),
    "Reference.pdf",
  );
  assert.deepEqual(await getWebBookThumbnail(library.library_id, contentHash), cover);
  const reopened = await openWebBook(library.library_id, contentHash);
  assert.ok(reopened instanceof Blob);
  assert.equal(reopened.type, "application/pdf");
  assert.deepEqual(new Uint8Array(await reopened.arrayBuffer()), bytes);

  await removeWebLibrary(library.library_id);
});

test("catalogue ownership persists with the imported browser publication", async () => {
  const library = await createWebLibrary(`Catalogue import ${crypto.randomUUID()}`);
  const file = new File([testEpub("Catalogue Book")], "Catalogue Book.epub", {
    type: "application/epub+zip",
  });
  const contentHash = await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    file,
    file.name,
  );
  await recordWebCatalogPublication(library.library_id, contentHash, {
    source_id: "opds-test",
    catalog_locator: "https://catalog.example/root",
    publication: { edition_id: "edition-one", work_id: "work-one" },
    download_url: "https://catalog.example/one.epub",
    book_format: "epub",
  });
  assert.deepEqual(await getOwnedWebCatalogPublications(library.library_id), [
    {
      source_id: "opds-test",
      publication: { edition_id: "edition-one", work_id: "work-one" },
    },
  ]);
  await removeWebLibrary(library.library_id);
});

test("catalogue provenance failure rolls back the book import", async () => {
  const library = await createWebLibrary(`Atomic catalogue ${crypto.randomUUID()}`);
  const file = new File(
    [testEpub("Atomic Catalogue Book")],
    "Atomic Catalogue Book.epub",
    { type: "application/epub+zip" },
  );
  await assert.rejects(
    importWebBook(
      { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
      file,
      file.name,
      {
        source_id: null as unknown as string,
        catalog_locator: "https://catalog.example/root",
        publication: { edition_id: "atomic-edition", work_id: null },
        download_url: "https://catalog.example/atomic.epub",
        book_format: "epub",
      },
    ),
  );
  assert.deepEqual((await getWebLibraryHome(library.library_id)).recently_added, []);
  assert.deepEqual(await getOwnedWebCatalogPublications(library.library_id), []);
  await removeWebLibrary(library.library_id);
});

test("local search separates title matches from distinct author matches", async () => {
  const library = await createWebLibrary(`Search test ${crypto.randomUUID()}`);
  const first = new File([testEpub("Power and Progress")], "Power and Progress.epub", {
    type: "application/epub+zip",
  });
  const second = new File([testEpub("Other Work")], "Other Work.epub", {
    type: "application/epub+zip",
  });
  await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    first,
    first.name,
  );
  await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    second,
    second.name,
  );

  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  db.exec({
    sql: "UPDATE books SET author = 'GÖRAN Smith' WHERE library_id = ?",
    bind: [library.library_id],
  });

  const title = await searchWebLibrary(library.library_id, "  power   ", 100);
  assert.deepEqual(
    title.title_matches.map(({ title }) => title),
    ["Power and Progress"],
  );
  assert.deepEqual(title.author_matches, []);

  const author = await searchWebLibrary(library.library_id, "goran smith", 100);
  assert.deepEqual(author.title_matches, []);
  assert.deepEqual(
    author.author_matches.map(({ title }) => title),
    ["Other Work", "Power and Progress"],
  );

  const distinct = await searchWebLibrary(library.library_id, "progress", 100);
  assert.deepEqual(
    distinct.title_matches.map(({ title }) => title),
    ["Power and Progress"],
  );
  assert.deepEqual(distinct.author_matches, []);
  await removeWebLibrary(library.library_id);
});

test("file type views prune empty folder and subject branches recursively", async () => {
  const library = await createWebLibrary(`File type filter ${crypto.randomUUID()}`);
  const bookFolder = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "Book branch",
  );
  const pdfFolder = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "PDF branch",
  );
  const audioFolder = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "Audio branch",
  );
  const bookHash = await importWebBook(
    { library_id: library.library_id, directory_id: bookFolder.id },
    new File([testEpub("Novel")], "Novel.epub"),
    "Novel.epub",
  );
  const pdfHash = await importWebBook(
    { library_id: library.library_id, directory_id: pdfFolder.id },
    new File([testEpub("Paper")], "Paper.epub"),
    "Paper.epub",
  );
  const audioHash = await importWebBook(
    { library_id: library.library_id, directory_id: audioFolder.id },
    new File([testEpub("Spoken")], "Spoken.epub"),
    "Spoken.epub",
  );
  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  db.exec({
    sql: "UPDATE books SET file_name='Paper.pdf' WHERE library_id=? AND content_hash=?",
    bind: [library.library_id, pdfHash],
  });
  db.exec({
    sql: "UPDATE books SET file_name='Spoken.m4b' WHERE library_id=? AND content_hash=?",
    bind: [library.library_id, audioHash],
  });
  for (const [hash, path] of [
    [bookHash, "Humanities / Fiction"],
    [pdfHash, "Science & Nature / Papers"],
    [audioHash, "Recreation / Spoken"],
  ]) {
    db.exec({
      sql: "INSERT INTO book_subject_paths(library_id,content_hash,subject_path) VALUES (?,?,?)",
      bind: [library.library_id, hash, path],
    });
  }

  assert.deepEqual(
    (
      await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID, "pdf")
    ).children.map(({ name, book_count }) => [name, book_count]),
    [["PDF branch", 1]],
  );
  assert.deepEqual(
    (
      await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID, "audiobook")
    ).children.map(({ name }) => name),
    ["Audio branch"],
  );
  assert.deepEqual(
    (
      await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID, "book")
    ).children.map(({ name }) => name),
    ["Book branch"],
  );
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, null, "pdf")).books.map(
      ({ title }) => title,
    ),
    ["Paper"],
  );
  assert.deepEqual(
    (
      await searchWebLibrarySubject(library.library_id, null, "paper", 10, "pdf")
    ).title_matches.map(({ title }) => title),
    ["Paper"],
  );
  assert.deepEqual(
    (await searchWebLibrarySubject(library.library_id, null, "novel", 10, "pdf"))
      .title_matches,
    [],
  );
  await removeWebLibrary(library.library_id);
});

test("folders, paths, authors and uncategorized subjects use stored library state", async () => {
  const library = await createWebLibrary(`Hierarchy test ${crypto.randomUUID()}`);
  const fiction = await createWebDirectory(
    library.library_id,
    ROOT_DIRECTORY_ID,
    "Fiction",
  );
  const classics = await createWebDirectory(library.library_id, fiction.id, "Classics");
  const file = new File([testEpub("Austen")], "Austen.epub", {
    type: "application/epub+zip",
  });
  await importWebBook(
    { library_id: library.library_id, directory_id: classics.id },
    file,
    file.name,
  );

  const root = await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID);
  assert.deepEqual(
    root.children.map(({ name, book_count }) => [name, book_count]),
    [["Fiction", 1]],
  );
  assert.equal(
    (await getWebLibraryFolder(library.library_id, classics.id)).books[0].title,
    "Austen",
  );
  assert.deepEqual(
    (await getWebDirectoryPath(library.library_id, classics.id)).map(({ name }) => name),
    ["Fiction", "Classics"],
  );
  assert.deepEqual(
    (
      await searchWebLibrary(library.library_id, "austen", 100, fiction.id)
    ).title_matches.map(({ title }) => title),
    ["Austen"],
  );

  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  const authorId = crypto.randomUUID();
  db.exec({
    sql: "UPDATE books SET author = 'Jane Austen', author_id = ?, author_key = 'jane austen' WHERE library_id = ?",
    bind: [authorId, library.library_id],
  });
  db.exec({
    sql: `INSERT INTO book_subject_paths(library_id,content_hash,subject_path)
          SELECT library_id,content_hash,'LCC / LCSH / English fiction' FROM books WHERE library_id=?`,
    bind: [library.library_id],
  });
  const authors = await getWebLibraryAuthors(library.library_id, null);
  assert.deepEqual(
    authors.authors.map(({ id, name, book_count }) => [id, name, book_count]),
    [[authorId, "Jane Austen", 1]],
  );
  assert.equal(
    (await getWebLibraryAuthors(library.library_id, authorId)).books?.[0].title,
    "Austen",
  );

  const subjects = await getWebLibrarySubjects(library.library_id, null);
  assert.deepEqual(
    subjects.children,
    [],
    "stored subjects from other systems are not exposed as a Subjects root",
  );
  assert.equal(
    (await getWebLibrarySubjects(library.library_id, "__uncategorized__")).books.length,
    0,
  );
  // Keep the hidden route readable so old sessions do not lose access and,
  // more importantly, prove that hiding the root did not discard metadata.
  assert.equal(
    (await getWebLibrarySubjects(library.library_id, "__other_subject_systems__"))
      .books[0].title,
    "Austen",
  );
  assert.equal(
    (
      await searchWebLibrarySubject(
        library.library_id,
        "__other_subject_systems__",
        "austen",
        10,
      )
    ).title_matches.length,
    1,
  );
  await removeWebLibrary(library.library_id);
});

test("EPUB descriptions and Rust-derived subjects populate the library hierarchy", async () => {
  const library = await createWebLibrary(`Metadata test ${crypto.randomUUID()}`);
  const bytes = storedZip({
    mimetype: "application/epub+zip",
    "META-INF/container.xml":
      '<container><rootfiles><rootfile full-path="OPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    "OPS/book.opf":
      "<package><metadata><dc:title>American Century</dc:title><dc:description>A history description</dc:description><dc:subject>History</dc:subject><dc:subject>United States</dc:subject><dc:subject>20th Century</dc:subject></metadata><manifest></manifest></package>",
  });
  const file = new File([bytes], "American Century.epub", {
    type: "application/epub+zip",
  });
  await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    file,
    file.name,
  );

  const home = await getWebLibraryHome(library.library_id);
  assert.equal(home.recently_added[0].description, "A history description");
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, null)).books.map(
      ({ title }) => title,
    ),
    ["American Century"],
  );
  assert.deepEqual(
    (await getWebLibrarySubjects(library.library_id, "Humanities")).books.map(
      ({ title }) => title,
    ),
    ["American Century"],
  );
  assert.deepEqual(
    (
      await getWebLibrarySubjects(
        library.library_id,
        "Humanities / History / History by Region / Americas / North America / United States / Historical Eras / 20th Century",
      )
    ).books.map(({ title }) => title),
    ["American Century"],
  );
  assert.deepEqual(
    (
      await searchWebLibrarySubject(
        library.library_id,
        "Humanities / History",
        "american",
        10,
      )
    ).title_matches.map(({ title }) => title),
    ["American Century"],
  );
  await removeWebLibrary(library.library_id);
});

test("subject browsing removes empty choices and collapses non-branching hierarchy", async () => {
  const library = await createWebLibrary(`Compact subjects ${crypto.randomUUID()}`);
  const austin = await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    new File([testEpub("Austin")], "Austin.epub"),
    "Austin.epub",
  );
  const dallas = await importWebBook(
    { library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID },
    new File([testEpub("Dallas")], "Dallas.epub"),
    "Dallas.epub",
  );
  const { db } = await import("./database.ts").then(({ getWebDatabase }) =>
    getWebDatabase(),
  );
  db.exec({
    sql: "INSERT INTO book_subject_paths(library_id,content_hash,subject_path) VALUES (?,?,?)",
    bind: [
      library.library_id,
      austin,
      "Geography & Travel / United States / Texas / Austin",
    ],
  });
  db.exec({
    sql: "INSERT INTO book_subject_paths(library_id,content_hash,subject_path) VALUES (?,?,?)",
    bind: [
      library.library_id,
      dallas,
      "Geography & Travel / United States / Texas / Dallas",
    ],
  });

  const root = await getWebLibrarySubjects(library.library_id, null);
  assert.deepEqual(
    root.children.map(({ id }) => id),
    ["Geography & Travel / United States / Texas"],
  );
  assert.deepEqual(root.books, []);
  const texas = await getWebLibrarySubjects(
    library.library_id,
    "Geography & Travel / United States / Texas",
  );
  assert.deepEqual(texas.children, []);
  assert.deepEqual(
    texas.books.map(({ title }) => title),
    ["Austin", "Dallas"],
  );
  await removeWebLibrary(library.library_id);
});
