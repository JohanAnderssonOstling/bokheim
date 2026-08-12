import { FRONTEND_CONTRACT, type AuthorSort, type AuthorSummary, type BookCardRow, type BookImportDestination, type Dir, type LibraryAuthorsView, type LibraryEntry, type LibraryHomeView, type LibraryHierarchyView, type LibrarySearchResult } from "../bindings.ts";
import { ROOT_DIRECTORY_ID } from "../directory-id.ts";
import { getWebDatabase } from "./database.ts";
import { inspectPublication } from "./publication-metadata.ts";
import { readPublication, removeLibraryPublications, removePublication, storePublication } from "./publication-storage.ts";
import { hashPublication, normalizeSearchText } from "./wasm-core.ts";

function entry(libraryId: string, libraryName: string): LibraryEntry {
  return {
    library_id: libraryId,
    library_name: libraryName,
    library_path: `opfs://bokheim/${libraryId}`,
  };
}

function enqueueSyncMutation(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  kind: string,
  entityKey: string,
  value: unknown,
  changedAt: number,
  entitySubkey = "",
): void {
  db.exec({
    sql: `INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?)
          ON CONFLICT (library_id) DO NOTHING`,
    bind: [libraryId, crypto.randomUUID()],
  });
  db.exec({ sql: "UPDATE web_sync_state SET last_replica_seq = last_replica_seq + 1 WHERE library_id = ?", bind: [libraryId] });
  const sequence = Number(db.exec({
    sql: "SELECT last_replica_seq FROM web_sync_state WHERE library_id = ?",
    bind: [libraryId], rowMode: 0, returnValue: "resultRows",
  })[0]);
  const mutationId = crypto.randomUUID();
  const lifecycle = value as { deleted?: unknown; purged?: unknown };
  const conflictRank = lifecycle.purged !== undefined ? 2
    : (kind === "placement" && (value as { present?: boolean }).present === false) || lifecycle.deleted !== undefined ? 1 : 0;
  db.exec({
    sql: `INSERT INTO web_sync_outbox
          (library_id, mutation_id, replica_seq, kind, entity_key, entity_subkey, value_json, changed_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    bind: [libraryId, mutationId, sequence, kind, entityKey, entitySubkey, JSON.stringify(value), changedAt],
  });
  db.exec({
    sql: `INSERT INTO web_sync_cells
            (library_id, kind, entity_key, entity_subkey, value_json, changed_at, conflict_rank, replica_id, replica_seq, mutation_id)
          VALUES (?, ?, ?, ?, ?, ?, ?, (SELECT replica_id FROM web_sync_state WHERE library_id = ?), ?, ?)
          ON CONFLICT (library_id, kind, entity_key, entity_subkey) DO UPDATE SET
            value_json=excluded.value_json, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank,
            replica_id=excluded.replica_id, replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id`,
    bind: [libraryId, kind, entityKey, entitySubkey, JSON.stringify(value), changedAt, conflictRank, libraryId, sequence, mutationId],
  });
}

export async function getWebPendingSyncMutationCount(libraryId: string): Promise<number> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  return Number(db.exec({
    sql: "SELECT count(*) FROM web_sync_outbox WHERE library_id = ?",
    bind: [libraryId], rowMode: 0, returnValue: "resultRows",
  })[0]);
}

export async function getWebSyncDetails(libraryId: string): Promise<{ pending: number; lastSyncedAt: number | null }> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const row = db.exec({
    sql: `SELECT (SELECT count(*) FROM web_sync_outbox WHERE library_id=?), last_synced_at
          FROM web_sync_state WHERE library_id=?`,
    bind: [libraryId, libraryId], rowMode: "array", returnValue: "resultRows",
  })[0] as [number, number | null] | undefined;
  return { pending: Number(row?.[0] ?? 0), lastSyncedAt: row?.[1] ?? null };
}

export async function getWebLibraryBookCount(libraryId: string): Promise<number> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  return Number(db.exec({
    sql: "SELECT count(*) FROM books WHERE library_id=?",
    bind: [libraryId], rowMode: 0, returnValue: "resultRows",
  })[0]);
}

export async function listWebLibraries(): Promise<LibraryEntry[]> {
  const { db } = await getWebDatabase();
  const initialized = db.exec({
    sql: "SELECT 1 FROM web_backend_metadata WHERE key = 'default_library_created'",
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!initialized.length) {
    const libraryId = crypto.randomUUID();
    db.exec("BEGIN IMMEDIATE");
    try {
      const count = Number(db.exec({ sql: "SELECT count(*) FROM libraries", rowMode: 0, returnValue: "resultRows" })[0]);
      if (count === 0) {
        db.exec({
          sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, 'My Library', ?)",
          bind: [libraryId, Date.now()],
        });
      }
      db.exec("INSERT INTO web_backend_metadata (key, value) VALUES ('default_library_created', '1')");
      db.exec("COMMIT");
    } catch (reason) {
      db.exec("ROLLBACK");
      throw reason;
    }
  }
  const rows = db.exec({
    sql: "SELECT library_id, library_name FROM libraries ORDER BY library_name COLLATE NOCASE, library_id",
    rowMode: "array",
    returnValue: "resultRows",
  });
  return rows.map(([libraryId, libraryName]) => entry(String(libraryId), String(libraryName)));
}

export async function createWebLibrary(name: string): Promise<LibraryEntry> {
  const libraryName = name.trim();
  if (!libraryName || libraryName.length > 255) throw new Error("Library names must contain between 1 and 255 characters");
  const libraryId = crypto.randomUUID();
  const { db } = await getWebDatabase();
  db.exec({
    sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, ?, ?)",
    bind: [libraryId, libraryName, Date.now()],
  });
  return entry(libraryId, libraryName);
}

export async function attachRemoteWebLibrary(libraryId: string, name: string): Promise<LibraryEntry> {
  const libraryName = name.trim();
  if (!libraryName || libraryName.length > 255) throw new Error("The remote library name is invalid");
  // Preserve the browser's local-first invariant even when account bootstrap
  // attaches remote entries before the ordinary library screen is loaded.
  await listWebLibraries();
  const { db } = await getWebDatabase();
  db.exec({
    sql: `INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, ?, ?)
          ON CONFLICT (library_id) DO UPDATE SET library_name = excluded.library_name`,
    bind: [libraryId, libraryName, Date.now()],
  });
  db.exec({
    sql: `INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?)
          ON CONFLICT (library_id) DO NOTHING`,
    bind: [libraryId, crypto.randomUUID()],
  });
  db.exec({
    sql: "INSERT INTO web_remote_libraries (library_id) VALUES (?) ON CONFLICT (library_id) DO NOTHING",
    bind: [libraryId],
  });
  return entry(libraryId, libraryName);
}

export async function removeWebLibrary(libraryId: string, recordDeletion = true): Promise<void> {
  const { db } = await getWebDatabase();
  db.exec("BEGIN IMMEDIATE");
  try {
    if (recordDeletion) db.exec({
      sql: "INSERT INTO web_library_tombstones (library_id, deleted_at) VALUES (?, ?) ON CONFLICT (library_id) DO UPDATE SET deleted_at=excluded.deleted_at",
      bind: [libraryId, Date.now()],
    });
    db.exec({ sql: "DELETE FROM libraries WHERE library_id = ?", bind: [libraryId] });
    if (db.changes() === 0) throw new Error("The library does not exist");
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  await removeLibraryPublications(libraryId).catch(() => undefined);
}

async function requireLibrary(libraryId: string): Promise<void> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT 1 FROM libraries WHERE library_id = ?",
    bind: [libraryId],
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The library does not exist");
}

export async function getWebLibraryHome(libraryId: string): Promise<LibraryHomeView> {
  await requireLibrary(libraryId);
  const books = await listBooks(libraryId);
  return { recently_read: books.filter(({ progress }) => progress !== null), recently_added: books };
}

export async function getWebLibraryHierarchy(libraryId: string): Promise<LibraryHierarchyView> {
  return getWebLibraryFolder(libraryId, ROOT_DIRECTORY_ID);
}

async function requireDirectory(libraryId: string, directoryId: string): Promise<void> {
  if (directoryId === ROOT_DIRECTORY_ID) return requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT 1 FROM directories WHERE library_id = ? AND directory_id = ?",
    bind: [libraryId, directoryId], rowMode: 0, returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The library folder does not exist");
}

export async function createWebDirectory(libraryId: string, parentId: string, name: string): Promise<Dir> {
  const directoryName = name.trim();
  if (!directoryName || directoryName.length > 255 || directoryName.includes("/") || directoryName.includes("\\")) {
    throw new Error("Folder names must contain between 1 and 255 path-safe characters");
  }
  await requireDirectory(libraryId, parentId);
  const directoryId = crypto.randomUUID();
  const { db } = await getWebDatabase();
  try {
    const changedAt = Date.now();
    db.exec("BEGIN IMMEDIATE");
    db.exec({
      sql: "INSERT INTO directories (library_id, directory_id, parent_id, name, created_at) VALUES (?, ?, ?, ?, ?)",
      bind: [libraryId, directoryId, parentId, directoryName, changedAt],
    });
    enqueueSyncMutation(db, libraryId, "directory_name", directoryId, directoryName, changedAt);
    enqueueSyncMutation(db, libraryId, "directory_parent", directoryId, parentId, changedAt);
    enqueueSyncMutation(db, libraryId, "directory_lifecycle", directoryId, "present", changedAt);
    db.exec("COMMIT");
  } catch (reason) {
    try { db.exec("ROLLBACK"); } catch { /* transaction may not have started */ }
    throw new Error("A folder with that name already exists here", { cause: reason });
  }
  return { id: directoryId, name: directoryName, book_count: 0, downloaded_book_count: 0 };
}

export async function getWebChildDirectories(libraryId: string, parentId: string): Promise<Dir[]> {
  await requireDirectory(libraryId, parentId);
  return getDirectoryChildren(libraryId, parentId);
}

async function getDirectoryChildren(libraryId: string, parentId: string): Promise<Dir[]> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `
      WITH RECURSIVE descendants(root_id, directory_id) AS (
        SELECT directory_id, directory_id FROM directories WHERE library_id = ? AND parent_id = ?
        UNION ALL
        SELECT descendants.root_id, child.directory_id
        FROM descendants
        JOIN directories child ON child.library_id = ? AND child.parent_id = descendants.directory_id
      )
      SELECT child.directory_id AS id, child.name,
             count(DISTINCT placement.content_hash) AS book_count
      FROM directories child
      LEFT JOIN descendants ON descendants.root_id = child.directory_id
      LEFT JOIN book_placements placement
        ON placement.library_id = child.library_id AND placement.directory_id = descendants.directory_id
      WHERE child.library_id = ? AND child.parent_id = ?
      GROUP BY child.directory_id, child.name
      ORDER BY child.name COLLATE NOCASE, child.directory_id
    `,
    bind: [libraryId, parentId, libraryId, libraryId, parentId],
    rowMode: "object", returnValue: "resultRows",
  });
  return rows.map((row) => ({
    id: String(row.id), name: String(row.name),
    book_count: Number(row.book_count), downloaded_book_count: Number(row.book_count),
  }));
}

export async function getWebLibraryFolder(libraryId: string, directoryId: string): Promise<LibraryHierarchyView> {
  await requireDirectory(libraryId, directoryId);
  return { children: await getDirectoryChildren(libraryId, directoryId), books: await listBooks(libraryId, directoryId) };
}

export async function getWebDirectoryPath(libraryId: string, directoryId: string): Promise<Dir[]> {
  await requireDirectory(libraryId, directoryId);
  if (directoryId === ROOT_DIRECTORY_ID) return [];
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `
      WITH RECURSIVE path(directory_id, parent_id, name, depth) AS (
        SELECT directory_id, parent_id, name, 0 FROM directories WHERE library_id = ? AND directory_id = ?
        UNION ALL
        SELECT parent.directory_id, parent.parent_id, parent.name, path.depth + 1
        FROM path JOIN directories parent ON parent.library_id = ? AND parent.directory_id = path.parent_id
      )
      SELECT directory_id, name FROM path ORDER BY depth DESC
    `,
    bind: [libraryId, directoryId, libraryId], rowMode: "array", returnValue: "resultRows",
  });
  return rows.map(([id, name]) => ({ id: String(id), name: String(name), book_count: 0, downloaded_book_count: 0 }));
}

export async function getWebLibrarySubjects(libraryId: string, subjectPath: string | null): Promise<LibraryHierarchyView> {
  await requireLibrary(libraryId);
  const books = await listBooks(libraryId);
  if (subjectPath === null) {
    return books.length ? {
      children: [{ id: "__uncategorized__", name: "Uncategorized", book_count: books.length, downloaded_book_count: books.length }],
      books: [],
    } : { children: [], books: [] };
  }
  return { children: [], books: subjectPath === "__uncategorized__" ? books : [] };
}

export async function getWebLibraryAuthors(libraryId: string, selectedAuthorId: string | null, sort: AuthorSort = "name"): Promise<LibraryAuthorsView> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    // The covers the index rows draw, and the filing name they sort by. The
    // web database keeps no `file-as`, so the display name is the sort name
    // here — the same fallback the backend applies.
    sql: `SELECT author_id AS id, min(author) AS name, count(*) AS book_count,
                 (SELECT group_concat(content_hash)
                    FROM (SELECT content_hash FROM books cover
                           WHERE cover.library_id = books.library_id AND cover.author_id = books.author_id
                           ORDER BY lower(coalesce(cover.title, '')), cover.content_hash
                           LIMIT ?)) AS cover_hashes
          FROM books WHERE library_id = ? AND author_id IS NOT NULL
          GROUP BY author_id
          ORDER BY ${sort === "book_count" ? "book_count DESC, " : ""}name COLLATE NOCASE, author_id`,
    bind: [FRONTEND_CONTRACT.browsing_layout.author_row_cover_count, libraryId], rowMode: "object", returnValue: "resultRows",
  });
  const authors: AuthorSummary[] = rows.map((row) => ({
    id: String(row.id),
    name: String(row.name),
    sort_name: String(row.name),
    book_count: Number(row.book_count),
    cover_hashes: String(row.cover_hashes ?? "").split(",").filter(Boolean),
  }));
  return { authors, books: selectedAuthorId === null ? null : await listBooks(libraryId, undefined, selectedAuthorId) };
}

function titleFromFileName(fileName: string): string {
  return fileName.replace(/\.[^.]+$/, "").trim() || "Untitled";
}

function bookRow(row: Record<string, unknown>): BookCardRow {
  return {
    content_hash: String(row.content_hash),
    title: String(row.title),
    author: String(row.author ?? ""),
    description: "",
    progress: row.progress === null ? null : Number(row.progress),
    downloaded: Boolean(row.downloaded),
    download_requested: false,
    audiobook: false,
    audiobook_duration_ms: null,
    audiobook_chapter_count: null,
  };
}

async function listBooks(libraryId: string, directoryId?: string, authorId?: string): Promise<BookCardRow[]> {
  const { db } = await getWebDatabase();
  const placementJoin = directoryId === undefined ? "" : "JOIN book_placements placement USING (library_id, content_hash)";
  const filters = ["books.library_id = ?"];
  const bind: string[] = [libraryId];
  if (directoryId !== undefined) { filters.push("placement.directory_id = ?"); bind.push(directoryId); }
  if (authorId !== undefined) { filters.push("books.author_id = ?"); bind.push(authorId); }
  const rows = db.exec({
    sql: `
      SELECT books.content_hash, books.title, books.author, reading_positions.progress,
             (books.storage_key IS NOT NULL OR books.bytes IS NOT NULL) AS downloaded
      FROM books
      ${placementJoin}
      LEFT JOIN reading_positions USING (library_id, content_hash)
      WHERE ${filters.join(" AND ")}
      ORDER BY books.added_at DESC, books.content_hash
    `,
    bind,
    rowMode: "object",
    returnValue: "resultRows",
  });
  return rows.map((row) => bookRow(row));
}

async function listBooksInDirectoryTree(libraryId: string, directoryId: string): Promise<BookCardRow[]> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `
      WITH RECURSIVE tree(directory_id) AS (
        SELECT ?
        UNION ALL
        SELECT child.directory_id FROM tree
        JOIN directories child ON child.library_id = ? AND child.parent_id = tree.directory_id
      )
      SELECT DISTINCT books.content_hash, books.title, books.author, reading_positions.progress,
             (books.storage_key IS NOT NULL OR books.bytes IS NOT NULL) AS downloaded
      FROM tree
      JOIN book_placements placement ON placement.library_id = ? AND placement.directory_id = tree.directory_id
      JOIN books USING (library_id, content_hash)
      LEFT JOIN reading_positions USING (library_id, content_hash)
      ORDER BY books.added_at DESC, books.content_hash
    `,
    bind: [directoryId, libraryId, libraryId], rowMode: "object", returnValue: "resultRows",
  });
  return rows.map((row) => bookRow(row));
}

export async function searchWebLibrary(libraryId: string, query: string, limit: number, directoryId?: string): Promise<LibrarySearchResult> {
  await requireLibrary(libraryId);
  const normalizedQuery = await normalizeSearchText(query);
  if (!normalizedQuery) return { title_matches: [], author_matches: [] };
  if (!Number.isSafeInteger(limit) || limit < 1) throw new Error("search limit must be a positive integer");

  const books = directoryId === undefined ? await listBooks(libraryId) : await listBooksInDirectoryTree(libraryId, directoryId);
  const normalizedBooks = await Promise.all(books.map(async (book) => ({
    book,
    title: await normalizeSearchText(book.title),
    author: await normalizeSearchText(book.author),
  })));
  normalizedBooks.sort((left, right) =>
    left.title.localeCompare(right.title) || left.book.content_hash.localeCompare(right.book.content_hash));
  const titleMatches = normalizedBooks.filter(({ title }) => title.includes(normalizedQuery)).map(({ book }) => book);
  const titleIds = new Set(titleMatches.map(({ content_hash }) => content_hash));
  const authorMatches = normalizedBooks.filter(({ book, author }) =>
    !titleIds.has(book.content_hash) && author.includes(normalizedQuery)).map(({ book }) => book);
  return {
    title_matches: titleMatches.slice(0, limit),
    author_matches: authorMatches.slice(0, Math.max(0, limit - titleMatches.length)),
  };
}

export async function importWebBook(destination: BookImportDestination, file: File, fileName: string): Promise<void> {
  await requireLibrary(destination.library_id);
  await requireDirectory(destination.library_id, destination.directory_id);
  const bytes = new Uint8Array(await file.arrayBuffer());
  const metadata = await inspectPublication(file, fileName, bytes);
  const author = metadata.author ?? "";
  const authorKey = author ? await normalizeSearchText(author) : "";
  const contentHash = await hashPublication(bytes);
  const stored = await storePublication(destination.library_id, contentHash, bytes);
  const { db } = await getWebDatabase();
  try {
    const existingAuthor = authorKey ? db.exec({
      sql: "SELECT author_id FROM books WHERE library_id = ? AND author_key = ? AND author_id IS NOT NULL LIMIT 1",
      bind: [destination.library_id, authorKey], rowMode: 0, returnValue: "resultRows",
    }) : [];
    const authorId = authorKey ? String(existingAuthor[0] ?? crypto.randomUUID()) : null;
    const changedAt = Date.now();
    db.exec("BEGIN IMMEDIATE");
    db.exec({
      sql: `
        INSERT INTO books (library_id, content_hash, file_name, title, author, author_id, author_key, cover_bytes, storage_key, bytes, added_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (library_id, content_hash) DO UPDATE SET
          file_name = excluded.file_name,
          title = excluded.title,
          author = excluded.author,
          author_id = excluded.author_id,
          author_key = excluded.author_key,
          cover_bytes = excluded.cover_bytes,
          storage_key = excluded.storage_key,
          bytes = excluded.bytes
      `,
      bind: [
        destination.library_id, contentHash, fileName,
        metadata.title ?? titleFromFileName(fileName), author, authorId, authorKey,
        metadata.cover, stored.storageKey, stored.fallbackBytes, changedAt,
      ],
    });
    db.exec({
      sql: `INSERT INTO book_placements (library_id, content_hash, directory_id, file_name, added_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT (library_id, content_hash, directory_id) DO UPDATE SET file_name = excluded.file_name`,
      bind: [destination.library_id, contentHash, destination.directory_id, fileName, changedAt],
    });
    const synchronizedMetadata = {
      title: metadata.title ?? titleFromFileName(fileName),
      contributors: author ? [{ contributor_id: authorId, name: author, sort_name: null, role: [97, 117, 116] }] : [],
      description: "",
      publication: {},
    };
    enqueueSyncMutation(db, destination.library_id, "book_lifecycle", contentHash, {
      present: { metadata: synchronizedMetadata, added_at: changedAt, format: "Epub" },
    }, changedAt);
    enqueueSyncMutation(db, destination.library_id, "metadata", contentHash, synchronizedMetadata, changedAt);
    enqueueSyncMutation(db, destination.library_id, "placement", contentHash, {
      present: true, origin_folder_id: null,
    }, changedAt, destination.directory_id);
    db.exec("COMMIT");
  } catch (reason) {
    try { db.exec("ROLLBACK"); } catch { /* transaction may not have started */ }
    if (stored.storageKey) await removePublication(stored.storageKey).catch(() => undefined);
    throw reason;
  }
}

export async function getWebBookThumbnail(libraryId: string, contentHash: string): Promise<Uint8Array> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT cover_bytes FROM books WHERE library_id = ? AND content_hash = ?",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The book does not exist");
  return rows[0] instanceof Uint8Array ? rows[0] : new Uint8Array();
}

export async function openWebBook(libraryId: string, contentHash: string): Promise<Blob> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT storage_key, bytes FROM books WHERE library_id = ? AND content_hash = ?",
    bind: [libraryId, contentHash],
    rowMode: "array",
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The book does not exist");
  const storageKey = rows[0][0];
  if (typeof storageKey === "string") return readPublication(storageKey);
  const bytes = rows[0][1];
  if (!(bytes instanceof Uint8Array)) throw new Error("The stored book is invalid");

  return new Blob([bytes.slice().buffer], { type: "application/octet-stream" });
}

export async function getWebBookFileName(libraryId: string, contentHash: string): Promise<string> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT file_name FROM books WHERE library_id = ? AND content_hash = ?",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The book does not exist");
  return String(rows[0]);
}

export async function getWebReadingPosition(libraryId: string, contentHash: string): Promise<string | null> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT cfi FROM reading_positions WHERE library_id = ? AND content_hash = ?",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  });
  return rows.length ? String(rows[0]) : null;
}

export async function updateWebReadingPosition(libraryId: string, contentHash: string, cfi: string, progress: number | null): Promise<void> {
  const { db } = await getWebDatabase();
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec({
      sql: `
        INSERT INTO reading_positions (library_id, content_hash, cfi, progress, modified_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT (library_id, content_hash) DO UPDATE SET
          cfi = excluded.cfi,
          progress = excluded.progress,
          modified_at = excluded.modified_at
      `,
      bind: [libraryId, contentHash, cfi, progress, changedAt],
    });
    enqueueSyncMutation(db, libraryId, "reading_position", contentHash, { location: cfi, progress: progress ?? 0 }, changedAt);
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}
