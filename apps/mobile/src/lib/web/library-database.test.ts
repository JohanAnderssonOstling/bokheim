import assert from "node:assert/strict";
import test from "node:test";
import { ROOT_DIRECTORY_ID } from "../directory-id.ts";

import {
  attachRemoteWebLibrary,
  createWebLibrary,
  createWebDirectory,
  getWebDirectoryPath,
  getWebLibraryAuthors,
  getWebLibraryFolder,
  getWebLibrarySubjects,
  getWebLibraryBookCount,
  getWebPendingSyncMutationCount,
  getWebBookFileName,
  getWebBookThumbnail,
  getWebLibraryHome,
  getWebReadingPosition,
  importWebBook,
  listWebLibraries,
  openWebBook,
  removeWebLibrary,
  searchWebLibrary,
  updateWebReadingPosition,
} from "./library-database.ts";

test("remote catalogue entries attach idempotently and update their name", async () => {
  const libraryId = crypto.randomUUID();
  await attachRemoteWebLibrary(libraryId, "Remote Library");
  await attachRemoteWebLibrary(libraryId, "Renamed Remotely");
  const matches = (await listWebLibraries()).filter(({ library_id }) => library_id === libraryId);
  assert.deepEqual(matches.map(({ library_name }) => library_name), ["Renamed Remotely"]);
  assert.equal(await getWebPendingSyncMutationCount(libraryId), 0);
  const { db } = await import("./database.ts").then(({ getWebDatabase }) => getWebDatabase());
  assert.equal(db.exec({ sql: "SELECT count(*) FROM web_remote_libraries WHERE library_id=?", bind: [libraryId], rowMode: 0, returnValue: "resultRows" })[0], 1);
  await removeWebLibrary(libraryId);
});

test("browser libraries can be created, listed, opened and removed", async () => {
  const initial = await listWebLibraries();
  assert.ok(initial.some(({ library_name }) => library_name === "My Library"));
  const name = `Browser library ${crypto.randomUUID()}`;
  const created = await createWebLibrary(name);
  assert.equal(created.library_name, name);
  assert.match(created.library_path, /^opfs:\/\/bokheim\//);
  assert.ok((await listWebLibraries()).some(({ library_id }) => library_id === created.library_id));
  assert.deepEqual(await getWebLibraryHome(created.library_id), { recently_read: [], recently_added: [] });
  assert.equal(await getWebLibraryBookCount(created.library_id), 0);
  await removeWebLibrary(created.library_id);
  assert.ok(!(await listWebLibraries()).some(({ library_id }) => library_id === created.library_id));
});

test("the core browser reader loop imports, reopens and tracks a book", async () => {
  const library = await createWebLibrary(`Reader test ${crypto.randomUUID()}`);
  const bytes = new TextEncoder().encode("%PDF-1.7\nreader test");
  const file = new File([bytes], "Example Book.pdf", { type: "application/pdf" });

  await importWebBook({ library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID }, file, file.name);
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 3);
  const home = await getWebLibraryHome(library.library_id);
  assert.equal(await getWebLibraryBookCount(library.library_id), 1);
  assert.equal(home.recently_added.length, 1);
  const book = home.recently_added[0];
  assert.equal(book.title, "Example Book");
  assert.equal(book.downloaded, true);
  assert.equal(await getWebBookFileName(library.library_id, book.content_hash), file.name);

  const { db } = await import("./database.ts").then(({ getWebDatabase }) => getWebDatabase());
  const storage = db.exec({
    sql: "SELECT storage_key, length(bytes) FROM books WHERE library_id = ? AND content_hash = ?",
    bind: [library.library_id, book.content_hash],
    rowMode: "array",
    returnValue: "resultRows",
  });
  assert.deepEqual(storage, [[null, bytes.length]]);
  assert.deepEqual(await getWebBookThumbnail(library.library_id, book.content_hash), new Uint8Array());
  const cover = new Uint8Array([0xff, 0xd8, 0xff, 0xd9]);
  db.exec({
    sql: "UPDATE books SET cover_bytes = ? WHERE library_id = ? AND content_hash = ?",
    bind: [cover, library.library_id, book.content_hash],
  });
  assert.deepEqual(await getWebBookThumbnail(library.library_id, book.content_hash), cover);

  const reopened = new Uint8Array(await (await openWebBook(library.library_id, book.content_hash)).arrayBuffer());
  assert.deepEqual(reopened, bytes);
  assert.equal(await getWebReadingPosition(library.library_id, book.content_hash), null);

  await updateWebReadingPosition(library.library_id, book.content_hash, "epubcfi(/6/4)", 0.25);
  assert.equal(await getWebPendingSyncMutationCount(library.library_id), 4);
  assert.equal(await getWebReadingPosition(library.library_id, book.content_hash), "epubcfi(/6/4)");
  assert.equal((await getWebLibraryHome(library.library_id)).recently_read[0].progress, 0.25);

  await removeWebLibrary(library.library_id);
});

test("invalid core publication files are rejected before storage", async () => {
  const library = await createWebLibrary(`Invalid import ${crypto.randomUUID()}`);
  const invalid = new File([new Uint8Array([1, 2, 3])], "broken.pdf", { type: "application/pdf" });
  await assert.rejects(
    importWebBook({ library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID }, invalid, invalid.name),
    /not a readable PDF/,
  );
  assert.deepEqual((await getWebLibraryHome(library.library_id)).recently_added, []);
  await removeWebLibrary(library.library_id);
});

test("local search separates title matches from distinct author matches", async () => {
  const library = await createWebLibrary(`Search test ${crypto.randomUUID()}`);
  const first = new File(["%PDF-1.7\none"], "Power and Progress.pdf", { type: "application/pdf" });
  const second = new File(["%PDF-1.7\ntwo"], "Other Work.pdf", { type: "application/pdf" });
  await importWebBook({ library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID }, first, first.name);
  await importWebBook({ library_id: library.library_id, directory_id: ROOT_DIRECTORY_ID }, second, second.name);

  const { db } = await import("./database.ts").then(({ getWebDatabase }) => getWebDatabase());
  db.exec({ sql: "UPDATE books SET author = 'GÖRAN Smith' WHERE library_id = ?", bind: [library.library_id] });

  const title = await searchWebLibrary(library.library_id, "  power   ", 100);
  assert.deepEqual(title.title_matches.map(({ title }) => title), ["Power and Progress"]);
  assert.deepEqual(title.author_matches, []);

  const author = await searchWebLibrary(library.library_id, "goran smith", 100);
  assert.deepEqual(author.title_matches, []);
  assert.deepEqual(author.author_matches.map(({ title }) => title), ["Other Work", "Power and Progress"]);

  const distinct = await searchWebLibrary(library.library_id, "progress", 100);
  assert.deepEqual(distinct.title_matches.map(({ title }) => title), ["Power and Progress"]);
  assert.deepEqual(distinct.author_matches, []);
  await removeWebLibrary(library.library_id);
});

test("folders, paths, authors and uncategorized subjects use stored library state", async () => {
  const library = await createWebLibrary(`Hierarchy test ${crypto.randomUUID()}`);
  const fiction = await createWebDirectory(library.library_id, ROOT_DIRECTORY_ID, "Fiction");
  const classics = await createWebDirectory(library.library_id, fiction.id, "Classics");
  const file = new File(["%PDF-1.7\nhierarchy"], "Austen.pdf", { type: "application/pdf" });
  await importWebBook({ library_id: library.library_id, directory_id: classics.id }, file, file.name);

  const root = await getWebLibraryFolder(library.library_id, ROOT_DIRECTORY_ID);
  assert.deepEqual(root.children.map(({ name, book_count }) => [name, book_count]), [["Fiction", 1]]);
  assert.equal((await getWebLibraryFolder(library.library_id, classics.id)).books[0].title, "Austen");
  assert.deepEqual((await getWebDirectoryPath(library.library_id, classics.id)).map(({ name }) => name), ["Fiction", "Classics"]);
  assert.deepEqual((await searchWebLibrary(library.library_id, "austen", 100, fiction.id)).title_matches.map(({ title }) => title), ["Austen"]);

  const { db } = await import("./database.ts").then(({ getWebDatabase }) => getWebDatabase());
  const authorId = crypto.randomUUID();
  db.exec({ sql: "UPDATE books SET author = 'Jane Austen', author_id = ?, author_key = 'jane austen' WHERE library_id = ?", bind: [authorId, library.library_id] });
  const authors = await getWebLibraryAuthors(library.library_id, null);
  assert.deepEqual(authors.authors.map(({ id, name, book_count }) => [id, name, book_count]), [[authorId, "Jane Austen", 1]]);
  assert.equal((await getWebLibraryAuthors(library.library_id, authorId)).books?.[0].title, "Austen");

  const subjects = await getWebLibrarySubjects(library.library_id, null);
  assert.deepEqual(subjects.children.map(({ id, book_count }) => [id, book_count]), [["__uncategorized__", 1]]);
  assert.equal((await getWebLibrarySubjects(library.library_id, "__uncategorized__")).books[0].title, "Austen");
  await removeWebLibrary(library.library_id);
});
