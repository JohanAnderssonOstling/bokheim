import type { LibraryCatalogueState, LibraryEntry, LibrarySyncStatus } from "../bindings";
import { authenticatedWebFetch, getWebAccountSession, WIRE_MEDIA_TYPE } from "./account-client";
import { attachRemoteWebLibrary, claimWebLibraryForAccount, getWebLibraryBookCount, getWebSyncDetails, listWebLibraries, removeWebLibrary } from "./library-database";
import { getWebDatabase } from "./database";
import { decodeAccountResponse, encodeAccountRequest } from "./wasm-core";

interface LibrarySummary {
  library_id: string;
  library_name: string;
}

async function decodeResponse<T>(response: Response, kind: number): Promise<T> {
  return decodeAccountResponse<T>(kind, new Uint8Array(await response.arrayBuffer()));
}

async function listRemoteLibraries(): Promise<LibrarySummary[]> {
  return decodeResponse(await authenticatedWebFetch("/api/libraries"), 4);
}

async function reconcileDeletedLibraries(userId: string): Promise<void> {
  const { db } = await getWebDatabase();
  const localTombstones = db.exec({
    sql: "SELECT library_id FROM web_library_tombstones WHERE account_id=? ORDER BY deleted_at",
    bind: [userId], rowMode: 0, returnValue: "resultRows",
  }).map(String);
  for (const libraryId of localTombstones) {
    await authenticatedWebFetch(`/api/libraries/${encodeURIComponent(libraryId)}`, { method: "DELETE" });
    db.exec({ sql: "DELETE FROM web_library_tombstones WHERE library_id=?", bind: [libraryId] });
  }

  const response = await authenticatedWebFetch("/api/libraries/deleted");
  const deleted = await decodeResponse<{ library_ids: string[] }>(response, 5);
  const localIds = new Set((await listWebLibraries()).map(({ library_id }) => library_id));
  for (const libraryId of deleted.library_ids) {
    if (localIds.has(libraryId)) await removeWebLibrary(libraryId, false);
  }
}

async function upsertRemoteLibrary(library: LibraryEntry): Promise<LibrarySummary> {
  const body = await encodeAccountRequest(7, {
    library_id: library.library_id,
    library_name: library.library_name,
  });
  return decodeResponse(await authenticatedWebFetch("/api/libraries", {
    method: "POST",
    headers: { "Content-Type": WIRE_MEDIA_TYPE },
    body,
  }), 6);
}

/**
 * Reconciles catalogue identity only. Book metadata and EPUB bytes are handled
 * by the subsequent event/blob synchronization stages.
 */
export async function reconcileWebLibraryCatalogue(): Promise<LibraryCatalogueState> {
  const session = await getWebAccountSession();
  if (!session) throw new Error("Sign in to synchronize libraries");

  // Claim guest libraries before any remote reconciliation. Once claimed,
  // logout and subsequent accounts cannot list, mutate, delete, or sync them.
  for (const library of await listWebLibraries()) {
    await claimWebLibraryForAccount(library.library_id, session.user_id);
  }
  await reconcileDeletedLibraries(session.user_id);
  const remote = await listRemoteLibraries();
  for (const library of remote) await attachRemoteWebLibrary(library.library_id, library.library_name, session.user_id);

  const local = await listWebLibraries();
  const remoteIds = new Set(remote.map(({ library_id }) => library_id));
  for (const library of local) {
    if (!remoteIds.has(library.library_id) && await getWebLibraryBookCount(library.library_id) > 0) {
      const enrolled = await upsertRemoteLibrary(library);
      await attachRemoteWebLibrary(enrolled.library_id, enrolled.library_name, session.user_id);
      remoteIds.add(enrolled.library_id);
    }
  }

  const libraries = await listWebLibraries();
  const syncStatuses: LibrarySyncStatus[] = await Promise.all(libraries.map(async (library) => {
    const [details, bookCount] = await Promise.all([getWebSyncDetails(library.library_id), getWebLibraryBookCount(library.library_id)]);
    return {
      library_id: library.library_id,
      library_name: library.library_name,
      state: remoteIds.has(library.library_id) ? "synced" : bookCount > 0 ? "remote_missing" : "local_only",
      server_url: session.server_url,
      account_id: session.user_id,
      has_unsynced_changes: details.pending > 0,
      last_synced_at: details.lastSyncedAt,
    };
  }));
  return { libraries, sync_statuses: syncStatuses };
}

export async function bootstrapWebRemoteLibraries(): Promise<LibraryEntry[]> {
  return (await reconcileWebLibraryCatalogue()).libraries;
}
