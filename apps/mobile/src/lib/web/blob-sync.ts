import { authenticatedWebFetch, WIRE_MEDIA_TYPE } from "./account-client.ts";
import { getWebDatabase } from "./database.ts";
import { readPublication, storePublication } from "./publication-storage.ts";
import { decodeBlobManifestResponse, encodeAccountRequest, hashPublication, inspectEpubPublication, normalizeSearchText } from "./wasm-core.ts";

interface BlobManifestResponse {
  owned: string[];
  claimed: string[];
  upload: string[];
  rejected: Array<{ content_hash: string; reason: string }>;
}

type TransferKind = "book" | "thumbnail";
type TransferDirection = "upload" | "download";

async function transferState(
  libraryId: string, contentHash: string, kind: TransferKind, direction: TransferDirection,
  state: "queued" | "running" | "completed" | "failed", error: string | null = null,
): Promise<void> {
  const { db } = await getWebDatabase();
  db.exec({
    sql: `INSERT INTO web_transfers (library_id, content_hash, kind, direction, state, attempts, last_error, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)
          ON CONFLICT (library_id, content_hash, kind, direction) DO UPDATE SET
            state=excluded.state,
            attempts=web_transfers.attempts + CASE WHEN excluded.state='running' THEN 1 ELSE 0 END,
            last_error=excluded.last_error, updated_at=excluded.updated_at`,
    bind: [libraryId, contentHash, kind, direction, state, state === "running" ? 1 : 0, error, Date.now()],
  });
}

async function localBookBytes(libraryId: string, contentHash: string): Promise<Uint8Array> {
  const { db } = await getWebDatabase();
  const row = db.exec({
    sql: "SELECT storage_key, bytes FROM books WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash], rowMode: "array", returnValue: "resultRows",
  })[0] as [string | null, Uint8Array | null] | undefined;
  if (!row) throw new Error("The local book disappeared during synchronization");
  if (row[0]) return new Uint8Array(await (await readPublication(row[0])).arrayBuffer());
  if (row[1] instanceof Uint8Array) return row[1].slice();
  throw new Error("The local EPUB is not downloaded");
}

async function negotiateLocalBooks(libraryId: string): Promise<BlobManifestResponse> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `SELECT content_hash, CASE WHEN bytes IS NOT NULL THEN length(bytes) ELSE NULL END
          FROM books WHERE library_id=? AND (storage_key IS NOT NULL OR bytes IS NOT NULL)`,
    bind: [libraryId], rowMode: "array", returnValue: "resultRows",
  }) as Array<[string, number | null]>;
  const blobs = [] as Array<{ content_hash: string; size_bytes: number }>;
  for (const [contentHash, inlineSize] of rows) {
    const size = inlineSize ?? (await localBookBytes(libraryId, contentHash)).byteLength;
    blobs.push({ content_hash: contentHash, size_bytes: size });
  }
  if (!blobs.length) return { owned: [], claimed: [], upload: [], rejected: [] };
  const body = await encodeAccountRequest(8, { blobs });
  const response = await authenticatedWebFetch(`/api/libraries/${encodeURIComponent(libraryId)}/blobs/negotiate`, {
    method: "POST", headers: { "Content-Type": WIRE_MEDIA_TYPE }, body,
  });
  return decodeBlobManifestResponse<BlobManifestResponse>(new Uint8Array(await response.arrayBuffer()), { blobs });
}

async function uploadBook(libraryId: string, contentHash: string): Promise<void> {
  await transferState(libraryId, contentHash, "book", "upload", "running");
  try {
    const bytes = await localBookBytes(libraryId, contentHash);
    await authenticatedWebFetch(`/api/libraries/${encodeURIComponent(libraryId)}/blobs/${contentHash}`, {
      method: "PUT", headers: { "Content-Type": "application/octet-stream" }, body: bytes,
    });
    await transferState(libraryId, contentHash, "book", "upload", "completed");
  } catch (reason) {
    await transferState(libraryId, contentHash, "book", "upload", "failed", reason instanceof Error ? reason.message : String(reason));
    throw reason;
  }
}

async function updateDownloadedBookMetadata(libraryId: string, contentHash: string, bytes: Uint8Array): Promise<void> {
  let metadata;
  try {
    metadata = await inspectEpubPublication(bytes);
  } catch {
    // A valid transfer should still be readable by Foliate even when the small
    // metadata inspector cannot understand a particular EPUB package document.
    return;
  }
  const title = metadata.title?.trim() || null;
  const author = metadata.author?.trim() || null;
  if (!title && !author && !metadata.cover?.byteLength) return;

  const { db } = await getWebDatabase();
  const row = db.exec({
    sql: "SELECT author_id FROM books WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash], rowMode: 0, returnValue: "resultRows",
  });
  const authorId = author ? String(row[0] || crypto.randomUUID()) : null;
  const authorKey = author ? await normalizeSearchText(author) : null;
  db.exec({
    sql: `UPDATE books SET
            title=COALESCE(?, title),
            file_name=CASE WHEN ? IS NULL THEN file_name ELSE ? || '.epub' END,
            author=COALESCE(?, author),
            author_id=CASE WHEN ? IS NULL THEN author_id ELSE ? END,
            author_key=CASE WHEN ? IS NULL THEN author_key ELSE ? END,
            cover_bytes=COALESCE(?, cover_bytes)
          WHERE library_id=? AND content_hash=?`,
    bind: [title, title, title, author, author, authorId, author, authorKey, metadata.cover, libraryId, contentHash],
  });
  if (title) {
    db.exec({
      sql: "UPDATE book_placements SET file_name=? WHERE library_id=? AND content_hash=?",
      bind: [`${title}.epub`, libraryId, contentHash],
    });
  }
}

export async function downloadWebBook(libraryId: string, contentHash: string): Promise<"Downloaded"> {
  await transferState(libraryId, contentHash, "book", "download", "running");
  try {
    const response = await authenticatedWebFetch(`/api/libraries/${encodeURIComponent(libraryId)}/blobs/${contentHash}`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (await hashPublication(bytes) !== contentHash) throw new Error("Downloaded EPUB did not match its content hash");
    const stored = await storePublication(libraryId, contentHash, bytes);
    const { db } = await getWebDatabase();
    db.exec({
      sql: "UPDATE books SET storage_key=?, bytes=? WHERE library_id=? AND content_hash=?",
      bind: [stored.storageKey, stored.fallbackBytes, libraryId, contentHash],
    });
    await updateDownloadedBookMetadata(libraryId, contentHash, bytes);
    await transferState(libraryId, contentHash, "book", "download", "completed");
    return "Downloaded";
  } catch (reason) {
    await transferState(libraryId, contentHash, "book", "download", "failed", reason instanceof Error ? reason.message : String(reason));
    throw reason;
  }
}

async function synchronizeThumbnails(libraryId: string): Promise<void> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT content_hash, cover_bytes, (storage_key IS NOT NULL OR bytes IS NOT NULL) FROM books WHERE library_id=?",
    bind: [libraryId], rowMode: "array", returnValue: "resultRows",
  }) as Array<[string, Uint8Array | null, number]>;
  for (const [contentHash, cover, localBook] of rows) {
    if (cover instanceof Uint8Array && cover.byteLength) {
      await transferState(libraryId, contentHash, "thumbnail", "upload", "running");
      try {
        await authenticatedWebFetch(`/api/libraries/${encodeURIComponent(libraryId)}/thumbnails/${contentHash}`, {
          method: "PUT", headers: { "Content-Type": "image/jpeg" }, body: cover,
        });
        await transferState(libraryId, contentHash, "thumbnail", "upload", "completed");
      } catch (reason) {
        await transferState(libraryId, contentHash, "thumbnail", "upload", "failed", reason instanceof Error ? reason.message : String(reason));
      }
    } else if (!localBook) {
      await transferState(libraryId, contentHash, "thumbnail", "download", "running");
      try {
        const response = await authenticatedWebFetch(`/api/libraries/${encodeURIComponent(libraryId)}/thumbnails/${contentHash}`);
        const bytes = new Uint8Array(await response.arrayBuffer());
        db.exec({ sql: "UPDATE books SET cover_bytes=? WHERE library_id=? AND content_hash=?", bind: [bytes, libraryId, contentHash] });
        await transferState(libraryId, contentHash, "thumbnail", "download", "completed");
      } catch (reason) {
        await transferState(libraryId, contentHash, "thumbnail", "download", "failed", reason instanceof Error ? reason.message : String(reason));
      }
    }
  }
}

export async function synchronizeWebLibraryBlobs(libraryId: string): Promise<void> {
  const manifest = await negotiateLocalBooks(libraryId);
  if (manifest.rejected.length) {
    throw new Error(`Server rejected EPUB transfer: ${manifest.rejected.map(({ reason }) => reason).join(", ")}`);
  }
  for (const contentHash of manifest.upload) await uploadBook(libraryId, contentHash);

  const { db } = await getWebDatabase();
  const missing = db.exec({
    sql: "SELECT content_hash FROM books WHERE library_id=? AND storage_key IS NULL AND bytes IS NULL ORDER BY content_hash",
    bind: [libraryId], rowMode: 0, returnValue: "resultRows",
  }).map(String);
  for (const contentHash of missing) await downloadWebBook(libraryId, contentHash);
  await synchronizeThumbnails(libraryId);
}

export async function getWebLocalStorageUsage(): Promise<Array<{ library_id: string; library_name: string; used_bytes: number }>> {
  const { db } = await getWebDatabase();
  const libraries = db.exec({ sql: "SELECT library_id, library_name FROM libraries ORDER BY library_name COLLATE NOCASE", rowMode: "array", returnValue: "resultRows" }) as Array<[string, string]>;
  const result = [];
  for (const [libraryId, libraryName] of libraries) {
    const hashes = db.exec({
      sql: "SELECT content_hash FROM books WHERE library_id=? AND (storage_key IS NOT NULL OR bytes IS NOT NULL)",
      bind: [libraryId], rowMode: 0, returnValue: "resultRows",
    }).map(String);
    let usedBytes = 0;
    for (const contentHash of hashes) usedBytes += (await localBookBytes(libraryId, contentHash)).byteLength;
    result.push({ library_id: libraryId, library_name: libraryName, used_bytes: usedBytes });
  }
  return result;
}

export async function getWebTransferStatuses(): Promise<Array<{ library_id: string; library_name: string; transfers: unknown[]; history: unknown[] }>> {
  const { db } = await getWebDatabase();
  const libraries = db.exec({ sql: "SELECT library_id, library_name FROM libraries ORDER BY library_name COLLATE NOCASE", rowMode: "array", returnValue: "resultRows" }) as Array<[string, string]>;
  return libraries.map(([libraryId, libraryName]) => {
    const rows = db.exec({
      sql: "SELECT content_hash, kind, direction, state, attempts, last_error, updated_at FROM web_transfers WHERE library_id=? ORDER BY updated_at DESC",
      bind: [libraryId], rowMode: "object", returnValue: "resultRows",
    }) as Array<Record<string, unknown>>;
    const statuses = rows.map((row) => ({
      id: `web:${libraryId}:${row.kind}:${row.direction}:${row.content_hash}`,
      kind: `${row.direction}_${row.kind}`,
      content_hash: String(row.content_hash), book_content_hash: row.kind === "thumbnail" ? String(row.content_hash) : null,
      path: null, state: String(row.state), origin: "background", attempts: Number(row.attempts),
      completed_items: row.state === "completed" ? 1 : 0, total_items: 1,
      last_error: row.last_error === null ? null : String(row.last_error), next_retry_at: null,
      created_at: new Date(Number(row.updated_at)).toISOString(), updated_at: new Date(Number(row.updated_at)).toISOString(),
    }));
    return {
      library_id: libraryId, library_name: libraryName,
      transfers: statuses.filter(({ state }) => state === "queued" || state === "running"),
      history: statuses.filter(({ state }) => state === "completed" || state === "failed"),
    };
  });
}
