import { authenticatedWebFetch, WIRE_MEDIA_TYPE } from "./account-client.ts";
import { getWebDatabase } from "./database.ts";
import { enrichWebLibraryAuthors, enrichWebLibraryRich } from "./library-database.ts";
import { hydrateMutationTaxonomy } from "./taxonomy.ts";
import {
  decodeSyncExchange,
  encodeSyncExchange,
  projectDirectories,
} from "./wasm-core.ts";

interface OutboxMutation {
  mutation_id: string;
  kind: string;
  entity_key: string;
  entity_subkey: string;
  value: unknown;
  changed_at: number;
  replica_seq: number;
}

interface RemoteMutation extends OutboxMutation {
  conflict_rank: number;
  replica_id: string;
  revision: number;
}

interface ExchangeResponse {
  accepted: string[];
  rejected: Array<[string, string, boolean]>;
  mutations: RemoteMutation[];
  next_cursor: SyncCursor;
  has_more: boolean;
}

interface SyncCursor {
  state_revision: number | null;
  reading_revision: number | null;
}

function remoteVersionWins(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  mutation: RemoteMutation,
): boolean {
  const rows = db.exec({
    sql: `SELECT changed_at, conflict_rank, replica_id, replica_seq, mutation_id FROM web_sync_cells
          WHERE library_id=? AND kind=? AND entity_key=? AND entity_subkey=?`,
    bind: [libraryId, mutation.kind, mutation.entity_key, mutation.entity_subkey],
    rowMode: "array",
    returnValue: "resultRows",
  }) as unknown[][];
  if (!rows.length) return true;
  const stored = rows[0] as [number, number, string, number, string];
  const incoming: [number, number, string, number, string] = [
    mutation.changed_at,
    mutation.conflict_rank,
    mutation.replica_id,
    mutation.replica_seq,
    mutation.mutation_id,
  ];
  if (incoming[0] !== stored[0]) return incoming[0] > stored[0];
  if (incoming[1] !== stored[1]) return incoming[1] > stored[1];
  if (incoming[2] !== stored[2]) return incoming[2] > stored[2];
  if (incoming[3] !== stored[3]) return incoming[3] > stored[3];
  if (incoming[4] !== stored[4]) return incoming[4] > stored[4];
  return false;
}

function syncCellValues(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  entityKey: string,
): Map<string, { value: unknown; changedAt: number }> {
  const rows = db.exec({
    sql: "SELECT kind, value_json, changed_at FROM web_sync_cells WHERE library_id=? AND entity_key=?",
    bind: [libraryId, entityKey],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string, number]>;
  return new Map(
    rows.map(([kind, value, changedAt]) => [
      kind,
      { value: JSON.parse(value), changedAt },
    ]),
  );
}

interface VersionedCell {
  value: unknown;
  changed_at: number;
  conflict_rank: number;
  replica_id: string;
  replica_seq: number;
  mutation_id: string;
}

async function projectAllDirectories(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
): Promise<void> {
  const rows = db.exec({
    sql: `SELECT name.entity_key,
                 name.value_json, name.changed_at, name.conflict_rank, name.replica_id, name.replica_seq, name.mutation_id,
                 parent.value_json, parent.changed_at, parent.conflict_rank, parent.replica_id, parent.replica_seq, parent.mutation_id,
                 lifecycle.value_json, lifecycle.changed_at, lifecycle.conflict_rank, lifecycle.replica_id, lifecycle.replica_seq, lifecycle.mutation_id
          FROM web_sync_cells name
          JOIN web_sync_cells parent ON parent.library_id=name.library_id AND parent.entity_key=name.entity_key
            AND parent.entity_subkey='' AND parent.kind='directory_parent'
          JOIN web_sync_cells lifecycle ON lifecycle.library_id=name.library_id AND lifecycle.entity_key=name.entity_key
            AND lifecycle.entity_subkey='' AND lifecycle.kind='directory_lifecycle'
          WHERE name.library_id=? AND name.kind='directory_name' AND name.entity_subkey=''`,
    bind: [libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as unknown[][];
  const versioned = (row: unknown[], offset: number): VersionedCell => ({
    value: JSON.parse(String(row[offset])),
    changed_at: Number(row[offset + 1]),
    conflict_rank: Number(row[offset + 2]),
    replica_id: String(row[offset + 3]),
    replica_seq: Number(row[offset + 4]),
    mutation_id: String(row[offset + 5]),
  });
  const intents = rows.map((row) => ({
    id: String(row[0]),
    name: versioned(row, 1),
    parent: versioned(row, 7),
    lifecycle: versioned(row, 13),
  }));
  const projection = await projectDirectories<{
    ordered_active: string[];
    suppressed: string[];
    names: Array<[string, string]>;
  }>(intents);
  const byId = new Map(intents.map((intent) => [intent.id, intent]));
  const names = new Map(projection.names);
  db.exec({ sql: "DELETE FROM directories WHERE library_id=?", bind: [libraryId] });
  db.exec({ sql: "DELETE FROM web_trash_folders WHERE library_id=?", bind: [libraryId] });
  for (const intent of intents) {
    if (intent.lifecycle.value !== "deleted") continue;
    db.exec({
      sql: "INSERT INTO web_trash_folders (library_id,directory_id,name,deleted_at) VALUES (?,?,?,?)",
      bind: [
        libraryId,
        intent.id,
        names.get(intent.id) ?? String(intent.name.value),
        intent.lifecycle.changed_at,
      ],
    });
  }
  for (const directoryId of projection.ordered_active) {
    const intent = byId.get(directoryId);
    if (!intent)
      throw new Error("Shared directory projection returned an unknown directory");
    db.exec({
      sql: "INSERT INTO directories (library_id, directory_id, parent_id, name, created_at) VALUES (?, ?, ?, ?, ?)",
      bind: [
        libraryId,
        directoryId,
        String(intent.parent.value),
        names.get(directoryId) ?? String(intent.name.value),
        Date.now(),
      ],
    });
  }
}

export async function reprojectWebDirectories(libraryId: string): Promise<void> {
  const { db } = await getWebDatabase();
  db.exec("BEGIN IMMEDIATE");
  try {
    await projectAllDirectories(db, libraryId);
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

function metadataFields(metadata: Record<string, unknown>): {
  title: string;
  author: string;
  authorId: string | null;
  authorKey: string;
} {
  const contributors = Array.isArray(metadata.contributors) ? metadata.contributors : [];
  const author = contributors.find((entry) => {
    if (!entry || typeof entry !== "object") return false;
    const role = (entry as Record<string, unknown>).role;
    return Array.isArray(role) && role.join(",") === "97,117,116";
  }) as Record<string, unknown> | undefined;
  const name = typeof author?.name === "string" ? author.name : "";
  return {
    title:
      typeof metadata.title === "string" && metadata.title ? metadata.title : "Untitled",
    author: name,
    authorId: typeof author?.contributor_id === "string" ? author.contributor_id : null,
    authorKey: name
      .normalize("NFD")
      .replace(/\p{Diacritic}/gu, "")
      .toLowerCase(),
  };
}

function metadataSubjectPaths(metadata: Record<string, unknown>): string[] {
  const publication = metadata.publication;
  if (!publication || typeof publication !== "object") return [];
  const paths = (publication as Record<string, unknown>).subject_paths;
  return Array.isArray(paths)
    ? paths.filter((path): path is string => typeof path === "string" && Boolean(path))
    : [];
}

function projectBook(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  contentHash: string,
): string | null {
  const cells = syncCellValues(db, libraryId, contentHash);
  const lifecycle = cells.get("book_lifecycle")?.value;
  const present =
    lifecycle && typeof lifecycle === "object"
      ? ((lifecycle as Record<string, unknown>).present as
          Record<string, unknown> | undefined)
      : undefined;
  if (lifecycle === "purged") {
    const storageKey = db.exec({
      sql: "SELECT storage_key FROM books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
      rowMode: 0,
      returnValue: "resultRows",
    })[0];
    db.exec({
      sql: "DELETE FROM web_trash_books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    db.exec({
      sql: "DELETE FROM books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    return typeof storageKey === "string" ? storageKey : null;
  }
  const deleted = lifecycle && typeof lifecycle === "object" && "deleted" in lifecycle;
  if (deleted) {
    const existing = db.exec({
      sql: "SELECT title,file_name FROM books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
      rowMode: "array",
      returnValue: "resultRows",
    })[0] as [string, string] | undefined;
    const metadata =
      (cells.get("metadata")?.value as Record<string, unknown> | undefined) ?? {};
    const title =
      existing?.[0] ?? (typeof metadata.title === "string" ? metadata.title : "Untitled");
    const fileName = existing?.[1] ?? `${title}.epub`;
    const format = fileName.includes(".")
      ? fileName.split(".").pop()!.toLowerCase()
      : "epub";
    db.exec({
      sql: `INSERT INTO web_trash_books (library_id,content_hash,title,format,deleted_at) VALUES (?,?,?,?,?)
            ON CONFLICT (library_id,content_hash) DO UPDATE SET title=excluded.title,format=excluded.format,deleted_at=excluded.deleted_at`,
      bind: [
        libraryId,
        contentHash,
        title,
        format,
        cells.get("book_lifecycle")?.changedAt ?? Date.now(),
      ],
    });
    db.exec({
      sql: "DELETE FROM book_placements WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    return null;
  }
  db.exec({
    sql: "DELETE FROM web_trash_books WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
  });
  const lifecycleMetadata = present?.metadata as Record<string, unknown> | undefined;
  const metadata =
    (cells.get("metadata")?.value as Record<string, unknown> | undefined) ??
    lifecycleMetadata ??
    {};
  const fields = metadataFields(metadata);
  const extension = present?.format === "Pdf" ? "pdf" : "epub";
  const projectedFileName = `${fields.title}.${extension}`;
  const changedAt = Number(
    present?.added_at ?? cells.get("book_lifecycle")?.changedAt ?? Date.now(),
  );
  db.exec({
    sql: `INSERT INTO books (library_id, content_hash, file_name, title, author, author_id, author_key, description, added_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
          ON CONFLICT (library_id, content_hash) DO UPDATE SET
            title=excluded.title, author=excluded.author, author_id=excluded.author_id, author_key=excluded.author_key, description=excluded.description`,
    bind: [
      libraryId,
      contentHash,
      projectedFileName,
      fields.title,
      fields.author,
      fields.authorId,
      fields.authorKey,
      typeof metadata.description === "string" ? metadata.description : "",
      changedAt,
    ],
  });
  db.exec({
    sql: "DELETE FROM book_subject_paths WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
  });
  for (const subjectPath of metadataSubjectPaths(metadata))
    db.exec({
      sql: "INSERT INTO book_subject_paths (library_id,content_hash,subject_path) VALUES (?,?,?)",
      bind: [libraryId, contentHash, subjectPath],
    });

  db.exec({
    sql: "DELETE FROM book_placements WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
  });
  const placements = db.exec({
    sql: "SELECT entity_subkey, value_json, changed_at FROM web_sync_cells WHERE library_id=? AND kind='placement' AND entity_key=?",
    bind: [libraryId, contentHash],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string, number]>;
  for (const [directoryId, json, placementChangedAt] of placements) {
    const value = JSON.parse(json) as { present?: boolean };
    if (!value.present) continue;
    db.exec({
      sql: `INSERT INTO book_placements (library_id, content_hash, directory_id, file_name, added_at)
            VALUES (?, ?, ?, ?, ?)`,
      bind: [libraryId, contentHash, directoryId, projectedFileName, placementChangedAt],
    });
  }

  const reading = cells.get("reading_position");
  if (reading) {
    const value = reading.value as Record<string, unknown>;
    db.exec({
      sql: `INSERT INTO reading_positions (library_id, content_hash, cfi, progress, modified_at) VALUES (?, ?, ?, ?, ?)
            ON CONFLICT (library_id, content_hash) DO UPDATE SET cfi=excluded.cfi, progress=excluded.progress, modified_at=excluded.modified_at`,
      bind: [
        libraryId,
        contentHash,
        String(value.location),
        Number(value.progress),
        reading.changedAt,
      ],
    });
  }
  return null;
}

function applyProjection(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  mutation: RemoteMutation,
): string | null {
  return ["book_lifecycle", "metadata", "placement", "reading_position"].includes(
    mutation.kind,
  )
    ? projectBook(db, libraryId, mutation.entity_key)
    : null;
}

function projectCatalogPublications(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
): void {
  db.exec({
    sql: "DELETE FROM web_catalog_publications WHERE library_id=?",
    bind: [libraryId],
  });
  const rows = db.exec({
    sql: "SELECT entity_key,value_json FROM web_sync_cells WHERE library_id=? AND kind='catalog_publication' ORDER BY entity_key,entity_subkey",
    bind: [libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string]>;
  for (const [sourceId, json] of rows) {
    const value = JSON.parse(json) as {
      publication?: { edition_id?: unknown; work_id?: unknown };
      content_hash?: unknown;
    };
    const editionId = value.publication?.edition_id;
    const workId = value.publication?.work_id;
    const contentHash = value.content_hash;
    if (
      typeof editionId !== "string" ||
      typeof contentHash !== "string" ||
      (workId !== null && workId !== undefined && typeof workId !== "string")
    )
      continue;
    db.exec({
      sql: `INSERT INTO web_catalog_publications (library_id,source_id,edition_id,work_id,content_hash)
            SELECT ?,?,?,?,? WHERE EXISTS (SELECT 1 FROM books WHERE library_id=? AND content_hash=?)
            ON CONFLICT (library_id,source_id,edition_id,work_id) DO UPDATE SET content_hash=excluded.content_hash`,
      bind: [
        libraryId,
        sourceId,
        editionId,
        workId ?? "",
        contentHash,
        libraryId,
        contentHash,
      ],
    });
  }
}

export async function applyWebSyncExchange(
  libraryId: string,
  response: ExchangeResponse,
): Promise<void> {
  const completed = [...response.accepted];
  const transientRejections: string[] = [];
  const permanentRejections: string[] = [];
  for (const [mutationId, reason, transient] of response.rejected) {
    if (!transient) completed.push(mutationId);
    if (transient) transientRejections.push(mutationId);
    else permanentRejections.push(`${mutationId}: ${reason}`);
  }

  const { db } = await getWebDatabase();
  const removedPublicationKeys: string[] = [];
  db.exec("BEGIN IMMEDIATE");
  try {
    let directoriesChanged = false;
    let catalogPublicationsChanged = false;
    for (const mutation of response.mutations.sort(
      (left, right) => left.revision - right.revision,
    )) {
      if (!remoteVersionWins(db, libraryId, mutation)) continue;
      db.exec({
        sql: `INSERT INTO web_sync_cells
                  (library_id, kind, entity_key, entity_subkey, value_json, changed_at, conflict_rank, replica_id, replica_seq, mutation_id)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT (library_id, kind, entity_key, entity_subkey) DO UPDATE SET
                  value_json=excluded.value_json, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank,
                  replica_id=excluded.replica_id, replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id`,
        bind: [
          libraryId,
          mutation.kind,
          mutation.entity_key,
          mutation.entity_subkey,
          JSON.stringify(mutation.value),
          mutation.changed_at,
          mutation.conflict_rank,
          mutation.replica_id,
          mutation.replica_seq,
          mutation.mutation_id,
        ],
      });
      directoriesChanged ||= mutation.kind.startsWith("directory_");
      catalogPublicationsChanged ||=
        mutation.kind === "catalog_publication" || mutation.kind === "book_lifecycle";
      const removedPublicationKey = applyProjection(db, libraryId, mutation);
      if (removedPublicationKey) removedPublicationKeys.push(removedPublicationKey);
    }
    if (directoriesChanged) await projectAllDirectories(db, libraryId);
    if (catalogPublicationsChanged) projectCatalogPublications(db, libraryId);
    for (const mutationId of completed)
      db.exec({
        sql: "DELETE FROM web_sync_outbox WHERE library_id=? AND mutation_id=?",
        bind: [libraryId, mutationId],
      });
    db.exec({
      sql: "UPDATE web_sync_state SET last_state_revision=?, last_reading_revision=?, last_synced_at=? WHERE library_id=?",
      bind: [
        response.next_cursor.state_revision,
        response.next_cursor.reading_revision,
        Date.now(),
        libraryId,
      ],
    });
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  if (removedPublicationKeys.length) {
    const { removePublication } = await import("./publication-storage.ts");
    await Promise.all(
      removedPublicationKeys.map((key) => removePublication(key).catch(() => undefined)),
    );
  }
  if (permanentRejections.length)
    throw new Error(
      `Server rejected synchronization changes: ${permanentRejections.join(", ")}`,
    );
  if (transientRejections.length)
    throw new Error(
      `Server deferred ${transientRejections.length} synchronization change(s); they remain queued`,
    );
}

export async function synchronizeWebLibrary(libraryId: string): Promise<void> {
  const { db } = await getWebDatabase();
  const storedTaxonomyRows = db.exec({
    sql: "SELECT kind,entity_key,value_json FROM web_sync_cells WHERE library_id=? AND kind IN ('metadata','book_lifecycle') ORDER BY entity_key,kind",
    bind: [libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string, string]>;
  if (storedTaxonomyRows.length) {
    const storedMutations = storedTaxonomyRows.map(([kind, entityKey, json]) => ({
      kind,
      entityKey,
      value: JSON.parse(json) as unknown,
    }));
    await hydrateMutationTaxonomy(storedMutations).catch((reason) =>
      console.warn(
        "Continuing synchronization without refreshing stored taxonomy detail",
        reason,
      ),
    );
    db.exec("BEGIN IMMEDIATE");
    try {
      for (const mutation of storedMutations)
        db.exec({
          sql: "UPDATE web_sync_cells SET value_json=? WHERE library_id=? AND kind=? AND entity_key=?",
          bind: [
            JSON.stringify(mutation.value),
            libraryId,
            mutation.kind,
            mutation.entityKey,
          ],
        });
      for (const entityKey of new Set(
        storedMutations.map((mutation) => mutation.entityKey),
      ))
        projectBook(db, libraryId, entityKey);
      db.exec("COMMIT");
    } catch (reason) {
      db.exec("ROLLBACK");
      throw reason;
    }
  }
  await enrichWebLibraryRich(libraryId, Number.MAX_SAFE_INTEGER, 30_000).catch((reason) =>
    console.warn("Continuing web library sync without rich metadata enrichment", reason),
  );
  await enrichWebLibraryAuthors(libraryId, Number.MAX_SAFE_INTEGER, 20_000).catch(
    (reason) =>
      console.warn("Continuing web library sync without author enrichment", reason),
  );
  db.exec({
    sql: "INSERT INTO web_sync_state (library_id, replica_id) VALUES (?, ?) ON CONFLICT (library_id) DO NOTHING",
    bind: [libraryId, crypto.randomUUID()],
  });
  let hasMore = true;
  let page = 0;
  while (hasMore) {
    if (++page > 10_000) throw new Error("Synchronization pagination did not terminate");
    const state = db.exec({
      sql: "SELECT replica_id, last_state_revision, last_reading_revision FROM web_sync_state WHERE library_id=?",
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    })[0] as [string, number | null, number | null];
    const cursor: SyncCursor = { state_revision: state[1], reading_revision: state[2] };
    const rows = db.exec({
      sql: `SELECT mutation_id, kind, entity_key, entity_subkey, value_json, changed_at, replica_seq
            FROM web_sync_outbox WHERE library_id=? ORDER BY replica_seq LIMIT 1000`,
      bind: [libraryId],
      rowMode: "object",
      returnValue: "resultRows",
    }) as Array<Record<string, unknown>>;
    const mutations: OutboxMutation[] = rows.map((row) => ({
      mutation_id: String(row.mutation_id),
      kind: String(row.kind),
      entity_key: String(row.entity_key),
      entity_subkey: String(row.entity_subkey),
      value: JSON.parse(String(row.value_json)),
      changed_at: Number(row.changed_at),
      replica_seq: Number(row.replica_seq),
    }));
    const body = await encodeSyncExchange({
      library_id: libraryId,
      replica_id: state[0],
      cursor,
      mutations,
    });
    const serverResponse = await authenticatedWebFetch("/api/sync/exchange", {
      method: "POST",
      headers: { "Content-Type": WIRE_MEDIA_TYPE },
      body,
    });
    const response = await decodeSyncExchange<ExchangeResponse>(
      new Uint8Array(await serverResponse.arrayBuffer()),
      {
        requested: mutations.map(({ mutation_id }) => mutation_id),
        cursor,
      },
    );
    await hydrateMutationTaxonomy(response.mutations).catch((reason) =>
      console.warn("Continuing synchronization without missing taxonomy detail", reason),
    );
    await applyWebSyncExchange(libraryId, response);
    hasMore = response.has_more || mutations.length === 1000;
  }
}

async function runAllWebLibrarySynchronization(): Promise<void> {
  const { synchronizeWebLibraryBlobs } = await import("./blob-sync.ts");
  const { db } = await getWebDatabase();
  const libraryIds = db.exec({
    sql: `SELECT remote.library_id FROM web_remote_libraries remote
          JOIN web_library_owners owner ON owner.library_id=remote.library_id
          JOIN web_account_profile profile ON profile.singleton=1 AND profile.user_id=owner.user_id
          ORDER BY remote.library_id`,
    rowMode: 0,
    returnValue: "resultRows",
  });
  for (const libraryId of libraryIds) {
    await synchronizeWebLibrary(String(libraryId));
    await synchronizeWebLibraryBlobs(String(libraryId));
  }
}

let synchronizationPass: Promise<void> | undefined;
let synchronizationRequested = false;

/** Coalesces concurrent callers so SQLite and upload manifests have one writer. */
export function synchronizeAllWebLibraries(): Promise<void> {
  synchronizationRequested = true;
  if (!synchronizationPass) {
    synchronizationPass = (async () => {
      while (synchronizationRequested) {
        synchronizationRequested = false;
        await runAllWebLibrarySynchronization();
      }
    })().finally(() => {
      synchronizationPass = undefined;
    });
  }
  return synchronizationPass;
}

export async function synchronizeAllWebLibraryMetadata(): Promise<void> {
  const { db } = await getWebDatabase();
  const libraryIds = db.exec({
    sql: `SELECT remote.library_id FROM web_remote_libraries remote
          JOIN web_library_owners owner ON owner.library_id=remote.library_id
          JOIN web_account_profile profile ON profile.singleton=1 AND profile.user_id=owner.user_id
          ORDER BY remote.library_id`,
    rowMode: 0,
    returnValue: "resultRows",
  });
  for (const libraryId of libraryIds) await synchronizeWebLibrary(String(libraryId));
}

export async function synchronizeAllWebLibraryBlobs(): Promise<void> {
  const { synchronizeWebLibraryBlobs } = await import("./blob-sync.ts");
  const { db } = await getWebDatabase();
  const libraryIds = db.exec({
    sql: `SELECT remote.library_id FROM web_remote_libraries remote
          JOIN web_library_owners owner ON owner.library_id=remote.library_id
          JOIN web_account_profile profile ON profile.singleton=1 AND profile.user_id=owner.user_id
          ORDER BY remote.library_id`,
    rowMode: 0,
    returnValue: "resultRows",
  });
  for (const libraryId of libraryIds) await synchronizeWebLibraryBlobs(String(libraryId));
}
