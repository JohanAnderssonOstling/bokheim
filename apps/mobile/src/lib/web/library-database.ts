import {
  FRONTEND_CONTRACT,
  type AudiobookChapter,
  type AuthorSort,
  type AuthorSummary,
  type BookCardRow,
  type BookDetail,
  type BookImportDestination,
  type CatalogImportRequest,
  type CatalogPublicationKey,
  type CatalogSubjectPreference,
  type Dir,
  type LibraryAuthorsView,
  type LibraryEntry,
  type LibraryFileTypeFilter,
  type LibraryFolderDestination,
  type LibraryHomeView,
  type LibraryHierarchyView,
  type LibrarySearchResult,
  type LibraryTrashView,
} from "../bindings.ts";
import { ROOT_DIRECTORY_ID } from "../library/directory-id.ts";
import { getWebDatabase } from "./database.ts";
import { inspectPublication } from "./publication-metadata.ts";
import {
  readPublication,
  removeLibraryPublications,
  removePublication,
  storePublication,
} from "./publication-storage.ts";
import { hydrateWebTaxonomy } from "./taxonomy.ts";
import {
  chooseLibraryName,
  hashPublication,
  normalizeSearchText,
  unifiedSubjectPaths,
} from "./wasm-core.ts";

const UNIFIED_SUBJECT_ROOTS = new Set([
  "Humanities",
  "Arts & Media",
  "Social Sciences",
  "Geography & Travel",
  "Science & Nature",
  "Applied Sciences & Technology",
  "Health & Medicine",
  "Education & Learning",
  "Home & Lifestyle",
  "Recreation",
  "Reference & Information",
  "Children & Young Adult",
]);

function isUnifiedSubjectPath(path: string): boolean {
  return UNIFIED_SUBJECT_ROOTS.has(path.split(" / ")[0]);
}

async function matchingFileTypeHashes(
  libraryId: string,
  fileType: LibraryFileTypeFilter,
): Promise<Set<string> | null> {
  if (fileType === "all") return null;
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT content_hash,file_name FROM books WHERE library_id=? AND NOT EXISTS (SELECT 1 FROM web_trash_books trash WHERE trash.library_id=books.library_id AND trash.content_hash=books.content_hash)",
    bind: [libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string]>;
  return new Set(
    rows
      .filter(([, fileName]) => {
        const extension = String(fileName).split(".").at(-1)?.toLowerCase() ?? "";
        if (fileType === "pdf") return extension === "pdf";
        if (fileType === "audiobook") return extension === "m4b";
        return extension !== "pdf" && extension !== "m4b";
      })
      .map(([contentHash]) => String(contentHash)),
  );
}

function retainMatchingBooks(
  books: BookCardRow[],
  matching: Set<string> | null,
): BookCardRow[] {
  return matching === null
    ? books
    : books.filter((book) => matching.has(book.content_hash));
}

function entry(libraryId: string, libraryName: string): LibraryEntry {
  return {
    library_id: libraryId,
    library_name: libraryName,
    library_path: `opfs://bokheim/${libraryId}`,
  };
}

async function authorityAuthorId(authority: string, externalId: string): Promise<string> {
  const namespace = Uint8Array.from([
    0x7f, 0xe7, 0x7e, 0x34, 0x29, 0x7f, 0x4a, 0xfa, 0x99, 0x8d, 0x2f, 0x86, 0xf0, 0x58,
    0x7b, 0x31,
  ]);
  const seed = new TextEncoder().encode(`external-author:${authority}:${externalId}`);
  const input = new Uint8Array(namespace.length + seed.length);
  input.set(namespace);
  input.set(seed, namespace.length);
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-1", input)).slice(0, 16);
  digest[6] = (digest[6] & 0x0f) | 0x50;
  digest[8] = (digest[8] & 0x3f) | 0x80;
  const hex = [...digest].map((value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function activeAccountUserId(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
): string | null {
  const value = db.exec({
    sql: "SELECT user_id FROM web_account_profile WHERE singleton=1",
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  return value === undefined ? null : String(value);
}

function assignLibraryOwner(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  userId: string,
): void {
  const existing = db.exec({
    sql: "SELECT user_id FROM web_library_owners WHERE library_id=?",
    bind: [libraryId],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  if (existing !== undefined && String(existing) !== userId) {
    throw new Error("The local library belongs to a different account");
  }
  db.exec({
    sql: "INSERT INTO web_library_owners (library_id,user_id) VALUES (?,?) ON CONFLICT (library_id) DO UPDATE SET user_id=excluded.user_id",
    bind: [libraryId, userId],
  });
}

export async function claimWebLibraryForAccount(
  libraryId: string,
  userId: string,
): Promise<void> {
  const { db } = await getWebDatabase();
  assignLibraryOwner(db, libraryId, userId);
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
  db.exec({
    sql: "UPDATE web_sync_state SET last_replica_seq = last_replica_seq + 1 WHERE library_id = ?",
    bind: [libraryId],
  });
  const sequence = Number(
    db.exec({
      sql: "SELECT last_replica_seq FROM web_sync_state WHERE library_id = ?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
  );
  const mutationId = crypto.randomUUID();
  const lifecycle = value as { deleted?: unknown; purged?: unknown };
  const conflictRank =
    value === "purged" || lifecycle.purged !== undefined
      ? 2
      : value === "deleted" ||
          (kind === "placement" && (value as { present?: boolean }).present === false) ||
          lifecycle.deleted !== undefined
        ? 1
        : 0;
  db.exec({
    sql: `INSERT INTO web_sync_outbox
          (library_id, mutation_id, replica_seq, kind, entity_key, entity_subkey, value_json, changed_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    bind: [
      libraryId,
      mutationId,
      sequence,
      kind,
      entityKey,
      entitySubkey,
      JSON.stringify(value),
      changedAt,
    ],
  });
  db.exec({
    sql: `INSERT INTO web_sync_cells
            (library_id, kind, entity_key, entity_subkey, value_json, changed_at, conflict_rank, replica_id, replica_seq, mutation_id)
          VALUES (?, ?, ?, ?, ?, ?, ?, (SELECT replica_id FROM web_sync_state WHERE library_id = ?), ?, ?)
          ON CONFLICT (library_id, kind, entity_key, entity_subkey) DO UPDATE SET
            value_json=excluded.value_json, changed_at=excluded.changed_at, conflict_rank=excluded.conflict_rank,
            replica_id=excluded.replica_id, replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id`,
    bind: [
      libraryId,
      kind,
      entityKey,
      entitySubkey,
      JSON.stringify(value),
      changedAt,
      conflictRank,
      libraryId,
      sequence,
      mutationId,
    ],
  });
}

export async function getWebPendingSyncMutationCount(libraryId: string): Promise<number> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  return Number(
    db.exec({
      sql: "SELECT count(*) FROM web_sync_outbox WHERE library_id = ?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
  );
}

interface OpenLibraryAuthorMetadata {
  open_library_author_id: string;
  name: string | null;
}

interface OpenLibraryMetadataResult {
  requested_isbn: string;
  canonical_isbn13: string | null;
  status: "matched" | "no_match" | "invalid_isbn" | "ambiguous";
  matches: Array<{ authors: OpenLibraryAuthorMetadata[] }>;
}

interface OpenLibraryMetadataResponse {
  results: OpenLibraryMetadataResult[];
}

interface RichEntityReference {
  wikidata_id: string | null;
  name: string;
  source: "exact_edition" | "work";
}
interface RichClassification {
  scheme: "library_of_congress" | "dewey_decimal";
  notation: string;
  source: "exact_edition" | "work";
}
interface RichMetadataMatch {
  classifications: RichClassification[];
  places: RichEntityReference[];
  people: RichEntityReference[];
}
interface RichMetadataResult {
  requested_isbn: string;
  canonical_isbn13: string | null;
  status: "matched" | "no_match" | "invalid_isbn" | "ambiguous";
  matches: RichMetadataMatch[];
}
interface RichPlaceEntity {
  wikidata_id: string;
  label: string;
  aliases: string[];
  parent_ids: string[];
  country_ids: string[];
  latitude_e7: number | null;
  longitude_e7: number | null;
}
interface RichPersonEntity {
  wikidata_id: string;
  label: string;
  aliases: string[];
  birth_year: number | null;
  death_year: number | null;
  fictional: boolean;
}
interface RichMetadataResponse {
  results: RichMetadataResult[];
  places: RichPlaceEntity[];
  people: RichPersonEntity[];
}

function unresolvedEntityId(kind: "place" | "person", name: string): string {
  const bytes = new TextEncoder().encode(name.trim());
  return `${kind === "place" ? "OLP" : "OLR"}-${[...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
}

function entityIdFromPath(path: string): string | null {
  return (
    path
      .split("/")
      .slice(1)
      .reverse()
      .find((id: string) => /^Q\d+$/.test(id) || /^OL[PR]-(?:[0-9a-f]{2})+$/i.test(id)) ??
    null
  );
}

/** Persists rich ISBN metadata and queues its canonical subject references before upload. */
export async function enrichWebLibraryRich(
  libraryId: string,
  limit = Number.MAX_SAFE_INTEGER,
  timeBudgetMs = 30_000,
): Promise<number> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const deadline = Date.now() + timeBudgetMs;
  const rows = db.exec({
    sql: `SELECT books.content_hash, MIN(identifier.value)
          FROM books
          JOIN web_book_identifiers identifier
            ON identifier.library_id=books.library_id AND identifier.content_hash=books.content_hash AND identifier.scheme='isbn'
          WHERE books.library_id=?
            AND NOT EXISTS (
              SELECT 1 FROM web_external_metadata_attempts attempt
              WHERE attempt.library_id=books.library_id AND attempt.content_hash=books.content_hash
                AND attempt.provider_id='openlibrary_rich_v2' AND attempt.attempted_at>=?
            )
          GROUP BY books.content_hash ORDER BY books.content_hash LIMIT ?`,
    bind: [libraryId, Date.now() - 30 * 24 * 60 * 60 * 1000, Math.min(limit, 0x7fffffff)],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string]>;
  let updated = 0;
  for (let offset = 0; offset < rows.length; offset += 256) {
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0) throw new Error("Open Library rich enrichment timed out");
    const batch = rows.slice(offset, offset + 256);
    const isbns = [...new Set(batch.map((row) => row[1]))];
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), Math.min(20_000, remainingMs));
    let response: RichMetadataResponse;
    try {
      const request = await fetch(
        "https://catalogs.bokheim.se/api/metadata/enrichment/v2",
        {
          method: "POST",
          headers: { "Content-Type": "application/json", Accept: "application/json" },
          body: JSON.stringify({ isbns }),
          signal: controller.signal,
        },
      );
      if (!request.ok)
        throw new Error(`Catalogue rich metadata server returned HTTP ${request.status}`);
      response = (await request.json()) as RichMetadataResponse;
    } finally {
      clearTimeout(timeout);
    }

    const ddcPaths = new Map<string, string[]>();
    const ddcNotations = [
      ...new Set(
        response.results
          .flatMap((result) => result.matches)
          .flatMap((matched) => matched.classifications)
          .filter((classification) => classification.scheme === "dewey_decimal")
          .map((classification) => classification.notation),
      ),
    ];
    const lccPaths = new Map<string, string[]>();
    const lccNotations = [
      ...new Set(
        response.results
          .flatMap((result) => result.matches)
          .flatMap((matched) => matched.classifications)
          .filter((classification) => classification.scheme === "library_of_congress")
          .map((classification) => classification.notation),
      ),
    ];
    await hydrateWebTaxonomy([
      ...ddcNotations.map((code) => ({ system: "ddc" as const, code })),
      ...lccNotations.map((code) => ({ system: "lcc" as const, code })),
    ]).catch((reason) =>
      console.warn("Continuing rich enrichment without missing taxonomy detail", reason),
    );
    await Promise.all(
      ddcNotations.map(async (notation) => {
        const paths = await unifiedSubjectPaths("ddc", notation);
        if (paths.length) ddcPaths.set(notation, paths);
      }),
    );
    await Promise.all(
      lccNotations.map(async (notation) => {
        const paths = await unifiedSubjectPaths("lcc", notation);
        if (paths.length) lccPaths.set(notation, paths);
      }),
    );

    db.exec("BEGIN IMMEDIATE");
    try {
      for (const entity of response.places)
        db.exec({
          sql: `INSERT INTO web_external_entities
                (library_id,wikidata_id,kind,label,aliases_json,latitude_e7,longitude_e7)
              VALUES (?,?,'place',?,?,?,?)
              ON CONFLICT (library_id,wikidata_id) DO UPDATE SET
                kind='place',label=excluded.label,aliases_json=excluded.aliases_json,
                latitude_e7=excluded.latitude_e7,longitude_e7=excluded.longitude_e7`,
          bind: [
            libraryId,
            entity.wikidata_id,
            entity.label,
            JSON.stringify(entity.aliases),
            entity.latitude_e7,
            entity.longitude_e7,
          ],
        });
      for (const entity of response.people)
        db.exec({
          sql: `INSERT INTO web_external_entities
                (library_id,wikidata_id,kind,label,aliases_json,birth_year,death_year,fictional)
              VALUES (?,?,'person',?,?,?,?,?)
              ON CONFLICT (library_id,wikidata_id) DO UPDATE SET
                kind='person',label=excluded.label,aliases_json=excluded.aliases_json,
                birth_year=excluded.birth_year,death_year=excluded.death_year,fictional=excluded.fictional`,
          bind: [
            libraryId,
            entity.wikidata_id,
            entity.label,
            JSON.stringify(entity.aliases),
            entity.birth_year,
            entity.death_year,
            entity.fictional ? 1 : 0,
          ],
        });
      for (const entity of response.places) {
        db.exec({
          sql: "DELETE FROM web_external_place_edges WHERE library_id=? AND child_id=?",
          bind: [libraryId, entity.wikidata_id],
        });
        for (const [relation, parents] of [
          ["parent", entity.parent_ids],
          ["country", entity.country_ids],
        ] as const) {
          for (const parentId of parents)
            db.exec({
              sql: "INSERT OR IGNORE INTO web_external_place_edges (library_id,child_id,parent_id,relation) VALUES (?,?,?,?)",
              bind: [libraryId, entity.wikidata_id, parentId, relation],
            });
        }
      }
      db.exec("COMMIT");
    } catch (reason) {
      db.exec("ROLLBACK");
      throw reason;
    }

    const byIsbn = new Map(
      response.results.map((result) => [result.requested_isbn, result]),
    );
    for (const [contentHash, isbn] of batch) {
      const result =
        byIsbn.get(isbn) ??
        response.results.find((candidate) => candidate.canonical_isbn13 === isbn);
      const matched =
        result?.status === "matched" && result.matches.length === 1
          ? result.matches[0]
          : null;
      const status =
        result === undefined
          ? "failed"
          : result.status === "no_match" || result.status === "invalid_isbn"
            ? "no_match"
            : matched
              ? "updated"
              : "ambiguous";
      const detail =
        result === undefined
          ? "Metadata response omitted the requested ISBN"
          : !matched && status === "ambiguous"
            ? "ISBN did not resolve to exactly one Open Library work"
            : null;
      const changedAt = Date.now();
      db.exec("BEGIN IMMEDIATE");
      try {
        db.exec({
          sql: "DELETE FROM web_book_external_entities WHERE library_id=? AND content_hash=?",
          bind: [libraryId, contentHash],
        });
        if (matched) {
          for (const [kind, references] of [
            ["place", matched.places],
            ["person", matched.people],
          ] as const) {
            for (const reference of references) {
              const entityId =
                reference.wikidata_id ?? unresolvedEntityId(kind, reference.name);
              if (!reference.wikidata_id)
                db.exec({
                  sql: `INSERT INTO web_external_entities (library_id,wikidata_id,kind,label,aliases_json)
                      VALUES (?,?,?,?, '[]') ON CONFLICT (library_id,wikidata_id) DO UPDATE SET kind=excluded.kind,label=excluded.label`,
                  bind: [libraryId, entityId, kind, reference.name],
                });
              db.exec({
                sql: `INSERT OR IGNORE INTO web_book_external_entities
                        (library_id,content_hash,wikidata_id,kind,evidence_name,source) VALUES (?,?,?,?,?,?)`,
                bind: [
                  libraryId,
                  contentHash,
                  entityId,
                  kind,
                  reference.name,
                  reference.source,
                ],
              });
            }
          }
          const rawMetadata = db.exec({
            sql: "SELECT value_json FROM web_sync_cells WHERE library_id=? AND kind='metadata' AND entity_key=? AND entity_subkey=''",
            bind: [libraryId, contentHash],
            rowMode: 0,
            returnValue: "resultRows",
          })[0];
          if (rawMetadata !== undefined) {
            const metadata = JSON.parse(String(rawMetadata)) as Record<string, unknown>;
            const publication =
              metadata.publication && typeof metadata.publication === "object"
                ? (metadata.publication as Record<string, unknown>)
                : {};
            const subjects = Array.isArray(publication.subjects)
              ? (publication.subjects as Array<Record<string, unknown>>)
              : [];
            const subjectPaths = Array.isArray(publication.subject_paths)
              ? publication.subject_paths.filter(
                  (path): path is string => typeof path === "string",
                )
              : [];
            const additions: Array<Record<string, unknown>> = [];
            for (const classification of matched.classifications)
              additions.push({
                id: null,
                name: classification.notation,
                source: `openlibrary:metadata:${classification.source.replace("_", "-")}`,
                authority:
                  classification.scheme === "library_of_congress" ? "lcc" : "ddc",
                code: classification.notation,
              });
            for (const [authority, references] of [
              ["wikidata-place", matched.places],
              ["wikidata-person", matched.people],
            ] as const) {
              for (const reference of references)
                additions.push({
                  id: null,
                  name: reference.name,
                  source: `openlibrary:metadata:${reference.source.replace("_", "-")}`,
                  authority,
                  code: reference.wikidata_id,
                });
            }
            for (const addition of additions)
              if (
                !subjects.some(
                  (subject) =>
                    subject.authority === addition.authority &&
                    subject.code === addition.code &&
                    subject.name === addition.name,
                )
              )
                subjects.push(addition);
            for (const classification of matched.classifications) {
              const paths =
                classification.scheme === "dewey_decimal"
                  ? (ddcPaths.get(classification.notation) ?? [])
                  : (lccPaths.get(classification.notation) ?? []);
              for (const path of paths)
                if (!subjectPaths.includes(path)) subjectPaths.push(path);
            }
            publication.subjects = subjects;
            publication.subject_paths = subjectPaths;
            metadata.publication = publication;
            db.exec({
              sql: "DELETE FROM book_subject_paths WHERE library_id=? AND content_hash=?",
              bind: [libraryId, contentHash],
            });
            for (const path of subjectPaths)
              db.exec({
                sql: "INSERT OR IGNORE INTO book_subject_paths (library_id,content_hash,subject_path) VALUES (?,?,?)",
                bind: [libraryId, contentHash, path],
              });
            enqueueSyncMutation(
              db,
              libraryId,
              "metadata",
              contentHash,
              metadata,
              changedAt,
            );
          }
          updated += 1;
        }
        db.exec({
          sql: `INSERT INTO web_external_metadata_attempts
                  (library_id,content_hash,provider_id,identifier,status,detail,attempted_at)
                VALUES (?,?,'openlibrary_rich_v2',?,?,?,?)
                ON CONFLICT (library_id,content_hash,provider_id) DO UPDATE SET
                  identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at`,
          bind: [libraryId, contentHash, isbn, status, detail, changedAt],
        });
        db.exec("COMMIT");
      } catch (reason) {
        db.exec("ROLLBACK");
        throw reason;
      }
    }
  }
  return updated;
}

/** Best-effort ISBN-only enrichment performed immediately before web sync. */
export async function enrichWebLibraryAuthors(
  libraryId: string,
  limit = Number.MAX_SAFE_INTEGER,
  timeBudgetMs = 20_000,
): Promise<number> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const deadline = Date.now() + timeBudgetMs;
  const rows = db.exec({
    sql: `SELECT books.content_hash, MIN(identifier.value), books.author, books.author_id
          FROM books
          JOIN web_book_identifiers identifier
            ON identifier.library_id=books.library_id AND identifier.content_hash=books.content_hash AND identifier.scheme='isbn'
          WHERE books.library_id=? AND books.author<>''
            AND NOT EXISTS (
              SELECT 1 FROM web_external_metadata_attempts attempt
              WHERE attempt.library_id=books.library_id AND attempt.content_hash=books.content_hash
                AND attempt.provider_id='openlibrary_authors'
                AND attempt.attempted_at>=?
            )
          GROUP BY books.content_hash
          ORDER BY books.content_hash
          LIMIT ?`,
    bind: [libraryId, Date.now() - 30 * 24 * 60 * 60 * 1000, Math.min(limit, 0x7fffffff)],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string, string, string | null]>;
  let updated = 0;
  for (let offset = 0; offset < rows.length; offset += 256) {
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0) throw new Error("Open Library author enrichment timed out");
    const batch = rows.slice(offset, offset + 256);
    const isbns = [...new Set(batch.map((row) => row[1]))];
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), Math.min(15_000, remainingMs));
    let response: OpenLibraryMetadataResponse;
    try {
      const request = await fetch("https://catalogs.bokheim.se/api/metadata/enrichment", {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({ isbns }),
        signal: controller.signal,
      });
      if (!request.ok)
        throw new Error(`Catalogue metadata server returned HTTP ${request.status}`);
      response = (await request.json()) as OpenLibraryMetadataResponse;
    } finally {
      clearTimeout(timeout);
    }
    const byIsbn = new Map(
      response.results.map((result) => [result.requested_isbn, result]),
    );
    for (const [contentHash, isbn, creditedName, provisionalAuthorId] of batch) {
      const result =
        byIsbn.get(isbn) ??
        response.results.find((candidate) => candidate.canonical_isbn13 === isbn);
      let status: "updated" | "no_match" | "ambiguous" | "failed" = "failed";
      let detail: string | null = "Metadata response omitted the requested ISBN";
      let resolved: { remote: OpenLibraryAuthorMetadata; preferredName: string } | null =
        null;
      if (result?.status === "no_match" || result?.status === "invalid_isbn") {
        status = "no_match";
        detail = null;
      } else if (result?.status === "matched" && result.matches.length === 1) {
        const localKey = await normalizeSearchText(creditedName);
        const matching: OpenLibraryAuthorMetadata[] = [];
        for (const author of result.matches[0].authors) {
          if (author.name && (await normalizeSearchText(author.name)) === localKey)
            matching.push(author);
        }
        if (matching.length === 1) {
          resolved = {
            remote: matching[0],
            preferredName: matching[0].name ?? creditedName,
          };
          status = "updated";
          detail = null;
        } else {
          status = "ambiguous";
          detail =
            "Open Library authors could not be matched uniquely to the scanned author credit";
        }
      } else if (result) {
        status = "ambiguous";
        detail = "ISBN did not resolve to exactly one Open Library work";
      }

      const authorityId = resolved
        ? await authorityAuthorId("open_library", resolved.remote.open_library_author_id)
        : null;

      const changedAt = Date.now();
      db.exec("BEGIN IMMEDIATE");
      try {
        if (resolved) {
          const existing = db.exec({
            sql: `SELECT author_id FROM web_author_external_identifiers
                  WHERE library_id=? AND authority='open_library' AND external_id=?`,
            bind: [libraryId, resolved.remote.open_library_author_id],
            rowMode: 0,
            returnValue: "resultRows",
          })[0];
          const authorId =
            existing === undefined
              ? (authorityId ?? provisionalAuthorId ?? crypto.randomUUID())
              : String(existing);
          db.exec({
            sql: `INSERT INTO web_author_external_identifiers (library_id,authority,external_id,author_id,preferred_name)
                  VALUES (?,'open_library',?,?,?)
                  ON CONFLICT (library_id,authority,external_id) DO UPDATE SET preferred_name=excluded.preferred_name`,
            bind: [
              libraryId,
              resolved.remote.open_library_author_id,
              authorId,
              resolved.preferredName,
            ],
          });
          db.exec({
            sql: "UPDATE books SET author_id=? WHERE library_id=? AND content_hash=?",
            bind: [authorId, libraryId, contentHash],
          });
          const rawMetadata = db.exec({
            sql: "SELECT value_json FROM web_sync_cells WHERE library_id=? AND kind='metadata' AND entity_key=? AND entity_subkey=''",
            bind: [libraryId, contentHash],
            rowMode: 0,
            returnValue: "resultRows",
          })[0];
          if (rawMetadata !== undefined) {
            const metadata = JSON.parse(String(rawMetadata)) as {
              contributors?: Array<Record<string, unknown>>;
            };
            const contributors = Array.isArray(metadata.contributors)
              ? metadata.contributors
              : [];
            const author = contributors.find(
              (contributor) =>
                Array.isArray(contributor.role) &&
                contributor.role.join(",") === "97,117,116",
            );
            if (author) author.contributor_id = authorId;
            enqueueSyncMutation(
              db,
              libraryId,
              "metadata",
              contentHash,
              metadata,
              changedAt,
            );
          }
          updated += 1;
        }
        db.exec({
          sql: `INSERT INTO web_external_metadata_attempts
                  (library_id,content_hash,provider_id,identifier,status,detail,attempted_at)
                VALUES (?,?,'openlibrary_authors',?,?,?,?)
                ON CONFLICT (library_id,content_hash,provider_id) DO UPDATE SET
                  identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at`,
          bind: [libraryId, contentHash, isbn, status, detail, changedAt],
        });
        db.exec("COMMIT");
      } catch (reason) {
        db.exec("ROLLBACK");
        throw reason;
      }
    }
  }
  return updated;
}

export async function getWebSyncDetails(
  libraryId: string,
): Promise<{ pending: number; lastSyncedAt: number | null }> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const row = db.exec({
    sql: `SELECT (SELECT count(*) FROM web_sync_outbox WHERE library_id=?), last_synced_at
          FROM web_sync_state WHERE library_id=?`,
    bind: [libraryId, libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  })[0] as [number, number | null] | undefined;
  return { pending: Number(row?.[0] ?? 0), lastSyncedAt: row?.[1] ?? null };
}

export async function getWebLibraryBookCount(libraryId: string): Promise<number> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  return Number(
    db.exec({
      sql: "SELECT count(*) FROM books WHERE library_id=?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
  );
}

export async function listWebLibraries(): Promise<LibraryEntry[]> {
  const { db } = await getWebDatabase();
  const userId = activeAccountUserId(db);
  const visibleRows = () =>
    db.exec({
      sql: `SELECT library.library_id, library.library_name
          FROM libraries library
          LEFT JOIN web_library_owners owner ON owner.library_id=library.library_id
          WHERE (? IS NULL AND owner.user_id IS NULL)
             OR (? IS NOT NULL AND (owner.user_id IS NULL OR owner.user_id=?))
          ORDER BY library.library_name COLLATE NOCASE, library.library_id`,
      bind: [userId, userId, userId],
      rowMode: "array",
      returnValue: "resultRows",
    });
  let rows = visibleRows();
  if (!rows.length) {
    const libraryId = crypto.randomUUID();
    db.exec("BEGIN IMMEDIATE");
    try {
      db.exec({
        sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, 'My Library', ?)",
        bind: [libraryId, Date.now()],
      });
      if (userId) assignLibraryOwner(db, libraryId, userId);
      db.exec("COMMIT");
    } catch (reason) {
      db.exec("ROLLBACK");
      throw reason;
    }
    rows = visibleRows();
  }
  return rows.map(([libraryId, libraryName]) =>
    entry(String(libraryId), String(libraryName)),
  );
}

export async function createWebLibrary(name: string): Promise<LibraryEntry> {
  const libraryName = name.trim();
  if (!libraryName || libraryName.length > 255)
    throw new Error("Library names must contain between 1 and 255 characters");
  const libraryId = crypto.randomUUID();
  const { db } = await getWebDatabase();
  db.exec({
    sql: "INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, ?, ?)",
    bind: [libraryId, libraryName, Date.now()],
  });
  const userId = activeAccountUserId(db);
  if (userId) assignLibraryOwner(db, libraryId, userId);
  return entry(libraryId, libraryName);
}

export async function attachRemoteWebLibrary(
  libraryId: string,
  name: string,
  userId?: string,
): Promise<LibraryEntry> {
  const libraryName = name.trim();
  if (!libraryName || libraryName.length > 255)
    throw new Error("The remote library name is invalid");
  const { db } = await getWebDatabase();
  if (userId) {
    const existingOwner = db.exec({
      sql: "SELECT user_id FROM web_library_owners WHERE library_id=?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0];
    if (existingOwner !== undefined && String(existingOwner) !== userId) {
      throw new Error("A remote library identifier conflicts with another local account");
    }
  }
  db.exec({
    sql: `INSERT INTO libraries (library_id, library_name, created_at) VALUES (?, ?, ?)
          ON CONFLICT (library_id) DO UPDATE SET library_name = excluded.library_name`,
    bind: [libraryId, libraryName, Date.now()],
  });
  if (userId) assignLibraryOwner(db, libraryId, userId);
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

export async function removeWebLibrary(
  libraryId: string,
  recordDeletion = true,
): Promise<void> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  db.exec("BEGIN IMMEDIATE");
  try {
    const owner = db.exec({
      sql: "SELECT user_id FROM web_library_owners WHERE library_id=?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0];
    if (recordDeletion && owner !== undefined)
      db.exec({
        sql: `INSERT INTO web_library_tombstones (library_id, deleted_at, account_id) VALUES (?, ?, ?)
            ON CONFLICT (library_id) DO UPDATE SET deleted_at=excluded.deleted_at,account_id=excluded.account_id`,
        bind: [libraryId, Date.now(), String(owner)],
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
  const userId = activeAccountUserId(db);
  const rows = db.exec({
    sql: `SELECT 1 FROM libraries library
          LEFT JOIN web_library_owners owner ON owner.library_id=library.library_id
          WHERE library.library_id=?
            AND ((? IS NULL AND owner.user_id IS NULL)
              OR (? IS NOT NULL AND (owner.user_id IS NULL OR owner.user_id=?)))`,
    bind: [libraryId, userId, userId, userId],
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The library does not exist");
}

export async function getWebLibraryHome(libraryId: string): Promise<LibraryHomeView> {
  await requireLibrary(libraryId);
  const books = await listBooks(libraryId);
  return {
    recently_read: books.filter(({ progress }) => progress !== null),
    recently_added: books,
  };
}

export async function getWebLibraryTrash(libraryId: string): Promise<LibraryTrashView> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const books = db
    .exec({
      sql: "SELECT content_hash,title,format,deleted_at FROM web_trash_books WHERE library_id=? ORDER BY deleted_at DESC,title COLLATE NOCASE",
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    })
    .map(([content_hash, title, format, deleted_at]) => ({
      content_hash: String(content_hash),
      title: String(title),
      format: String(format),
      deleted_at: Number(deleted_at),
    }));
  const folders = db
    .exec({
      sql: "SELECT directory_id,name,deleted_at FROM web_trash_folders WHERE library_id=? ORDER BY deleted_at DESC,name COLLATE NOCASE",
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    })
    .map(([id, name, deleted_at]) => ({
      id: String(id),
      name: String(name),
      deleted_at: Number(deleted_at),
    }));
  return { books, folders };
}

export async function moveWebBookToTrash(
  libraryId: string,
  contentHash: string,
): Promise<void> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    const placementIds = db
      .exec({
        sql: "SELECT directory_id FROM book_placements WHERE library_id=? AND content_hash=?",
        bind: [libraryId, contentHash],
        rowMode: 0,
        returnValue: "resultRows",
      })
      .map(String);
    db.exec({
      sql: `INSERT INTO web_trash_books (library_id,content_hash,title,format,deleted_at)
            SELECT library_id,content_hash,title,
                   lower(CASE WHEN instr(file_name,'.') > 0 THEN substr(file_name,instr(file_name,'.')+1) ELSE '' END),?
            FROM books WHERE library_id=? AND content_hash=?
            ON CONFLICT (library_id,content_hash) DO NOTHING`,
      bind: [changedAt, libraryId, contentHash],
    });
    if (db.changes() === 0)
      throw new Error("The book does not exist or is already in Trash");
    for (const placementDirectoryId of placementIds) {
      enqueueSyncMutation(
        db,
        libraryId,
        "placement",
        contentHash,
        { present: false, origin_folder_id: null },
        changedAt,
        placementDirectoryId,
      );
    }
    db.exec({
      sql: "DELETE FROM book_placements WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    enqueueSyncMutation(
      db,
      libraryId,
      "book_lifecycle",
      contentHash,
      { deleted: { origin_folder_id: null } },
      changedAt,
    );
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export async function moveWebFolderToTrash(
  libraryId: string,
  directoryId: string,
): Promise<void> {
  if (directoryId === ROOT_DIRECTORY_ID)
    throw new Error("The library root cannot be moved to Trash");
  await requireDirectory(libraryId, directoryId);
  const { db } = await getWebDatabase();
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    const placements = db.exec({
      sql: `WITH RECURSIVE subtree(directory_id) AS (
              SELECT ? UNION ALL
              SELECT child.directory_id FROM directories child JOIN subtree ON child.parent_id=subtree.directory_id
              WHERE child.library_id=? AND child.directory_id!=child.parent_id
            )
            SELECT placement.content_hash,placement.directory_id
            FROM book_placements placement JOIN subtree USING (directory_id)
            WHERE placement.library_id=?`,
      bind: [directoryId, libraryId, libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    }) as Array<[string, string]>;
    db.exec({
      sql: `INSERT INTO web_trash_folders (library_id,directory_id,name,deleted_at)
            SELECT library_id,directory_id,name,? FROM directories WHERE library_id=? AND directory_id=?
            ON CONFLICT (library_id,directory_id) DO NOTHING`,
      bind: [changedAt, libraryId, directoryId],
    });
    if (db.changes() === 0)
      throw new Error("The folder does not exist or is already in Trash");
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_lifecycle",
      directoryId,
      "deleted",
      changedAt,
    );
    for (const [contentHash, placementDirectoryId] of placements) {
      enqueueSyncMutation(
        db,
        libraryId,
        "placement",
        contentHash,
        { present: false, origin_folder_id: directoryId },
        changedAt,
        placementDirectoryId,
      );
      db.exec({
        sql: "DELETE FROM book_placements WHERE library_id=? AND content_hash=? AND directory_id=?",
        bind: [libraryId, contentHash, placementDirectoryId],
      });
    }
    for (const contentHash of new Set(placements.map(([hash]) => hash))) {
      const activePlacements = Number(
        db.exec({
          sql: "SELECT count(*) FROM book_placements WHERE library_id=? AND content_hash=?",
          bind: [libraryId, contentHash],
          rowMode: 0,
          returnValue: "resultRows",
        })[0],
      );
      const alreadyTrashed = Number(
        db.exec({
          sql: "SELECT count(*) FROM web_trash_books WHERE library_id=? AND content_hash=?",
          bind: [libraryId, contentHash],
          rowMode: 0,
          returnValue: "resultRows",
        })[0],
      );
      if (activePlacements || alreadyTrashed) continue;
      db.exec({
        sql: `INSERT INTO web_trash_books (library_id,content_hash,title,format,deleted_at)
              SELECT library_id,content_hash,title,
                     lower(CASE WHEN instr(file_name,'.') > 0 THEN substr(file_name,instr(file_name,'.')+1) ELSE '' END),?
              FROM books WHERE library_id=? AND content_hash=?`,
        bind: [changedAt, libraryId, contentHash],
      });
      enqueueSyncMutation(
        db,
        libraryId,
        "book_lifecycle",
        contentHash,
        { deleted: { origin_folder_id: directoryId } },
        changedAt,
      );
    }
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

function presentBookLifecycle(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  contentHash: string,
): unknown {
  const metadataJson = db.exec({
    sql: "SELECT value_json FROM web_sync_cells WHERE library_id=? AND kind='metadata' AND entity_key=? AND entity_subkey=''",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  const row = db.exec({
    sql: "SELECT title,author,author_id,added_at,file_name FROM books WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
    rowMode: "array",
    returnValue: "resultRows",
  })[0] as [string, string, string | null, number, string] | undefined;
  if (!row) throw new Error("The book no longer exists");
  const metadata =
    metadataJson === undefined
      ? {
          title: row[0],
          contributors: row[1]
            ? [
                {
                  contributor_id: row[2] ?? crypto.randomUUID(),
                  name: row[1],
                  sort_name: null,
                  role: [97, 117, 116],
                },
              ]
            : [],
          description: "",
          publication: {},
        }
      : JSON.parse(String(metadataJson));
  return {
    present: {
      metadata,
      added_at: row[3],
      format: /\.pdf$/i.test(row[4]) ? "Pdf" : "Epub",
    },
  };
}

export async function restoreWebTrashedBook(
  libraryId: string,
  contentHash: string,
): Promise<void> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    const lifecycle = presentBookLifecycle(db, libraryId, contentHash);
    db.exec({
      sql: "DELETE FROM web_trash_books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    if (db.changes() === 0) throw new Error("The trashed book does not exist");
    enqueueSyncMutation(
      db,
      libraryId,
      "book_lifecycle",
      contentHash,
      lifecycle,
      changedAt,
    );
    const removedPlacements = db
      .exec({
        sql: `SELECT entity_subkey FROM web_sync_cells
            WHERE library_id=? AND kind='placement' AND entity_key=?
              AND json_extract(value_json,'$.present')=0 AND json_extract(value_json,'$.origin_folder_id') IS NULL`,
        bind: [libraryId, contentHash],
        rowMode: 0,
        returnValue: "resultRows",
      })
      .map(String);
    const fileName = db.exec({
      sql: "SELECT file_name FROM books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
      rowMode: 0,
      returnValue: "resultRows",
    })[0];
    for (const placementDirectoryId of removedPlacements) {
      enqueueSyncMutation(
        db,
        libraryId,
        "placement",
        contentHash,
        { present: true, origin_folder_id: null },
        changedAt,
        placementDirectoryId,
      );
      db.exec({
        sql: `INSERT INTO book_placements (library_id,content_hash,directory_id,file_name,added_at) VALUES (?,?,?,?,?)
              ON CONFLICT (library_id,content_hash,directory_id) DO UPDATE SET file_name=excluded.file_name`,
        bind: [libraryId, contentHash, placementDirectoryId, String(fileName), changedAt],
      });
    }
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export async function purgeWebTrashedBook(
  libraryId: string,
  contentHash: string,
): Promise<void> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const storageKey = db.exec({
    sql: "SELECT storage_key FROM books WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec({
      sql: "DELETE FROM web_trash_books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    if (db.changes() === 0) throw new Error("The trashed book does not exist");
    enqueueSyncMutation(
      db,
      libraryId,
      "book_lifecycle",
      contentHash,
      "purged",
      changedAt,
    );
    db.exec({
      sql: "DELETE FROM books WHERE library_id=? AND content_hash=?",
      bind: [libraryId, contentHash],
    });
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  if (typeof storageKey === "string")
    await removePublication(storageKey).catch(() => undefined);
}

export async function restoreWebTrashedFolder(
  libraryId: string,
  directoryId: string,
): Promise<string> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const name = db.exec({
    sql: "SELECT name FROM web_trash_folders WHERE library_id=? AND directory_id=?",
    bind: [libraryId, directoryId],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  if (name === undefined) throw new Error("The trashed folder does not exist");
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec({
      sql: "DELETE FROM web_trash_folders WHERE library_id=? AND directory_id=?",
      bind: [libraryId, directoryId],
    });
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_lifecycle",
      directoryId,
      "present",
      changedAt,
    );
    const removedPlacements = db.exec({
      sql: `SELECT entity_key,entity_subkey FROM web_sync_cells
            WHERE library_id=? AND kind='placement' AND json_extract(value_json,'$.present')=0
              AND json_extract(value_json,'$.origin_folder_id')=?`,
      bind: [libraryId, directoryId],
      rowMode: "array",
      returnValue: "resultRows",
    }) as Array<[string, string]>;
    for (const [contentHash, placementDirectoryId] of removedPlacements) {
      enqueueSyncMutation(
        db,
        libraryId,
        "placement",
        contentHash,
        { present: true, origin_folder_id: null },
        changedAt,
        placementDirectoryId,
      );
      const fileName = db.exec({
        sql: "SELECT file_name FROM books WHERE library_id=? AND content_hash=?",
        bind: [libraryId, contentHash],
        rowMode: 0,
        returnValue: "resultRows",
      })[0];
      if (fileName !== undefined)
        db.exec({
          sql: `INSERT INTO book_placements (library_id,content_hash,directory_id,file_name,added_at) VALUES (?,?,?,?,?)
              ON CONFLICT (library_id,content_hash,directory_id) DO UPDATE SET file_name=excluded.file_name`,
          bind: [
            libraryId,
            contentHash,
            placementDirectoryId,
            String(fileName),
            changedAt,
          ],
        });
    }
    const orphanedBooks = db
      .exec({
        sql: `SELECT entity_key FROM web_sync_cells
            WHERE library_id=? AND kind='book_lifecycle'
              AND json_extract(value_json,'$.deleted.origin_folder_id')=?`,
        bind: [libraryId, directoryId],
        rowMode: 0,
        returnValue: "resultRows",
      })
      .map(String);
    for (const contentHash of orphanedBooks) {
      enqueueSyncMutation(
        db,
        libraryId,
        "book_lifecycle",
        contentHash,
        presentBookLifecycle(db, libraryId, contentHash),
        changedAt,
      );
      db.exec({
        sql: "DELETE FROM web_trash_books WHERE library_id=? AND content_hash=?",
        bind: [libraryId, contentHash],
      });
    }
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  await import("./library-sync.ts").then(({ reprojectWebDirectories }) =>
    reprojectWebDirectories(libraryId),
  );
  return String(name);
}

export async function purgeWebTrashedFolder(
  libraryId: string,
  directoryId: string,
): Promise<void> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const exists = db.exec({
    sql: "SELECT 1 FROM web_trash_folders WHERE library_id=? AND directory_id=?",
    bind: [libraryId, directoryId],
    rowMode: 0,
    returnValue: "resultRows",
  }).length;
  if (!exists) throw new Error("The trashed folder does not exist");
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    const subtree = db
      .exec({
        sql: `WITH RECURSIVE subtree(directory_id) AS (
              SELECT ? UNION ALL
              SELECT child.directory_id FROM directories child JOIN subtree ON child.parent_id=subtree.directory_id
              WHERE child.library_id=? AND child.directory_id!=child.parent_id
            ) SELECT directory_id FROM subtree`,
        bind: [directoryId, libraryId],
        rowMode: 0,
        returnValue: "resultRows",
      })
      .map(String);
    for (const purgedDirectoryId of subtree)
      enqueueSyncMutation(
        db,
        libraryId,
        "directory_lifecycle",
        purgedDirectoryId,
        "purged",
        changedAt,
      );
    for (const purgedDirectoryId of subtree.reverse()) {
      db.exec({
        sql: "DELETE FROM web_trash_folders WHERE library_id=? AND directory_id=?",
        bind: [libraryId, purgedDirectoryId],
      });
      db.exec({
        sql: "DELETE FROM directories WHERE library_id=? AND directory_id=?",
        bind: [libraryId, purgedDirectoryId],
      });
    }
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export async function emptyWebLibraryTrash(libraryId: string): Promise<void> {
  const trash = await getWebLibraryTrash(libraryId);
  for (const folder of trash.folders) await purgeWebTrashedFolder(libraryId, folder.id);
  for (const book of trash.books) await purgeWebTrashedBook(libraryId, book.content_hash);
}

export async function getWebLibraryHierarchy(
  libraryId: string,
): Promise<LibraryHierarchyView> {
  return getWebLibraryFolder(libraryId, ROOT_DIRECTORY_ID);
}

async function requireDirectory(libraryId: string, directoryId: string): Promise<void> {
  if (directoryId === ROOT_DIRECTORY_ID) return requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `WITH RECURSIVE ancestors(directory_id,parent_id) AS (
            SELECT directory_id,parent_id FROM directories WHERE library_id=? AND directory_id=?
            UNION ALL
            SELECT parent.directory_id,parent.parent_id FROM directories parent JOIN ancestors child ON child.parent_id=parent.directory_id
            WHERE parent.library_id=? AND parent.directory_id!=parent.parent_id
          )
          SELECT 1 FROM ancestors
          WHERE NOT EXISTS (
            SELECT 1 FROM ancestors path JOIN web_trash_folders trash
              ON trash.library_id=? AND trash.directory_id=path.directory_id
          ) LIMIT 1`,
    bind: [libraryId, directoryId, libraryId, libraryId],
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The library folder does not exist");
}

export async function createWebDirectory(
  libraryId: string,
  parentId: string,
  name: string,
): Promise<Dir> {
  const directoryName = name.trim();
  if (
    !directoryName ||
    directoryName.length > 255 ||
    directoryName.includes("/") ||
    directoryName.includes("\\")
  ) {
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
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_name",
      directoryId,
      directoryName,
      changedAt,
    );
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_parent",
      directoryId,
      parentId,
      changedAt,
    );
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_lifecycle",
      directoryId,
      "present",
      changedAt,
    );
    db.exec("COMMIT");
  } catch (reason) {
    try {
      db.exec("ROLLBACK");
    } catch {
      /* transaction may not have started */
    }
    throw new Error("A folder with that name already exists here", { cause: reason });
  }
  return {
    id: directoryId,
    name: directoryName,
    book_count: 0,
    downloaded_book_count: 0,
  };
}

export async function getWebFolderDestinations(
  libraryId: string,
): Promise<LibraryFolderDestination[]> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `WITH RECURSIVE tree(directory_id,path) AS (
            SELECT ?, ''
            UNION ALL
            SELECT child.directory_id,
                   CASE WHEN tree.path='' THEN child.name ELSE tree.path || ' / ' || child.name END
            FROM directories child JOIN tree ON child.parent_id=tree.directory_id
            WHERE child.library_id=? AND child.directory_id!=child.parent_id
              AND NOT EXISTS (SELECT 1 FROM web_trash_folders trash WHERE trash.library_id=child.library_id AND trash.directory_id=child.directory_id)
          ) SELECT directory_id,path FROM tree ORDER BY path COLLATE NOCASE,directory_id`,
    bind: [ROOT_DIRECTORY_ID, libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string]>;
  return rows.map(([id, path]) => ({
    id: String(id),
    path: path ? String(path) : "Library",
  }));
}

function validFolderName(name: string): string {
  const trimmed = name.trim();
  if (
    !trimmed ||
    trimmed.length > 255 ||
    trimmed.includes("/") ||
    trimmed.includes("\\")
  ) {
    throw new Error("Folder names must contain between 1 and 255 path-safe characters");
  }
  return trimmed;
}

export async function renameWebFolder(
  libraryId: string,
  directoryId: string,
  name: string,
): Promise<string> {
  if (directoryId === ROOT_DIRECTORY_ID)
    throw new Error("The library root cannot be renamed");
  const folderName = validFolderName(name);
  await requireDirectory(libraryId, directoryId);
  const { db } = await getWebDatabase();
  const parentId = String(
    db.exec({
      sql: "SELECT parent_id FROM directories WHERE library_id=? AND directory_id=?",
      bind: [libraryId, directoryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
  );
  const siblingNames = db
    .exec({
      sql: "SELECT name FROM directories WHERE library_id=? AND parent_id=? AND directory_id!=?",
      bind: [libraryId, parentId, directoryId],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String);
  const chosenName = await chooseLibraryName("folder", folderName, siblingNames);
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec({
      sql: "UPDATE directories SET name=? WHERE library_id=? AND directory_id=?",
      bind: [chosenName, libraryId, directoryId],
    });
    if (db.changes() === 0) throw new Error("The folder no longer exists");
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_name",
      directoryId,
      chosenName,
      changedAt,
    );
    db.exec("COMMIT");
    return chosenName;
  } catch (reason) {
    db.exec("ROLLBACK");
    if (reason instanceof Error && reason.message === "The folder no longer exists")
      throw reason;
    throw new Error("A folder with that name already exists here", { cause: reason });
  }
}

export async function moveWebFolder(
  libraryId: string,
  directoryId: string,
  destinationId: string,
): Promise<string> {
  if (directoryId === ROOT_DIRECTORY_ID)
    throw new Error("The library root cannot be moved");
  if (directoryId === destinationId)
    throw new Error("A folder cannot be moved into itself");
  await Promise.all([
    requireDirectory(libraryId, directoryId),
    requireDirectory(libraryId, destinationId),
  ]);
  const { db } = await getWebDatabase();
  const descendants = db
    .exec({
      sql: `WITH RECURSIVE subtree(directory_id) AS (
            SELECT ? UNION ALL SELECT child.directory_id FROM directories child JOIN subtree ON child.parent_id=subtree.directory_id
            WHERE child.library_id=? AND child.directory_id!=child.parent_id
          ) SELECT directory_id FROM subtree`,
      bind: [directoryId, libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String);
  if (descendants.includes(destinationId))
    throw new Error("A folder cannot be moved into itself or one of its descendants");
  const currentName = String(
    db.exec({
      sql: "SELECT name FROM directories WHERE library_id=? AND directory_id=?",
      bind: [libraryId, directoryId],
      rowMode: 0,
      returnValue: "resultRows",
    })[0],
  );
  const destinationNames = db
    .exec({
      sql: "SELECT name FROM directories WHERE library_id=? AND parent_id=? AND directory_id!=?",
      bind: [libraryId, destinationId, directoryId],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String);
  const chosenName = await chooseLibraryName("folder", currentName, destinationNames);
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec({
      sql: "UPDATE directories SET parent_id=?,name=? WHERE library_id=? AND directory_id=?",
      bind: [destinationId, chosenName, libraryId, directoryId],
    });
    if (db.changes() === 0) throw new Error("The folder no longer exists");
    enqueueSyncMutation(
      db,
      libraryId,
      "directory_parent",
      directoryId,
      destinationId,
      changedAt,
    );
    if (chosenName !== currentName)
      enqueueSyncMutation(
        db,
        libraryId,
        "directory_name",
        directoryId,
        chosenName,
        changedAt,
      );
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  return chosenName;
}

async function transferWebBookPlacement(
  libraryId: string,
  contentHash: string,
  sourceId: string,
  destinationId: string,
  removeSource: boolean,
): Promise<string> {
  if (sourceId === destinationId)
    throw new Error("Source and destination folders are the same");
  await Promise.all([
    requireDirectory(libraryId, sourceId),
    requireDirectory(libraryId, destinationId),
  ]);
  const { db } = await getWebDatabase();
  const sourceName = db.exec({
    sql: "SELECT file_name FROM book_placements WHERE library_id=? AND content_hash=? AND directory_id=?",
    bind: [libraryId, contentHash, sourceId],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  if (sourceName === undefined)
    throw new Error("The book is not present in the source folder");
  const existingDestination = db.exec({
    sql: "SELECT file_name FROM book_placements WHERE library_id=? AND content_hash=? AND directory_id=?",
    bind: [libraryId, contentHash, destinationId],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  let destinationName =
    existingDestination === undefined ? "" : String(existingDestination);
  if (existingDestination === undefined) {
    const names = db
      .exec({
        sql: "SELECT file_name FROM book_placements WHERE library_id=? AND directory_id=?",
        bind: [libraryId, destinationId],
        rowMode: 0,
        returnValue: "resultRows",
      })
      .map(String);
    destinationName = await chooseLibraryName("file", String(sourceName), names);
  }
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    if (existingDestination === undefined) {
      db.exec({
        sql: "INSERT INTO book_placements (library_id,content_hash,directory_id,file_name,added_at) VALUES (?,?,?,?,?)",
        bind: [libraryId, contentHash, destinationId, destinationName, changedAt],
      });
      enqueueSyncMutation(
        db,
        libraryId,
        "placement",
        contentHash,
        { present: true, origin_folder_id: null },
        changedAt,
        destinationId,
      );
    }
    if (removeSource) {
      db.exec({
        sql: "DELETE FROM book_placements WHERE library_id=? AND content_hash=? AND directory_id=?",
        bind: [libraryId, contentHash, sourceId],
      });
      enqueueSyncMutation(
        db,
        libraryId,
        "placement",
        contentHash,
        { present: false, origin_folder_id: null },
        changedAt,
        sourceId,
      );
    }
    db.exec("COMMIT");
    return destinationName;
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export function copyWebBookToFolder(
  libraryId: string,
  contentHash: string,
  sourceId: string,
  destinationId: string,
): Promise<string> {
  return transferWebBookPlacement(libraryId, contentHash, sourceId, destinationId, false);
}

export function moveWebBookToFolder(
  libraryId: string,
  contentHash: string,
  sourceId: string,
  destinationId: string,
): Promise<string> {
  return transferWebBookPlacement(libraryId, contentHash, sourceId, destinationId, true);
}

export async function removeWebBookFromFolder(
  libraryId: string,
  contentHash: string,
  sourceId: string,
): Promise<boolean> {
  await requireDirectory(libraryId, sourceId);
  const { db } = await getWebDatabase();
  const changedAt = Date.now();
  db.exec("BEGIN IMMEDIATE");
  try {
    db.exec({
      sql: "DELETE FROM book_placements WHERE library_id=? AND content_hash=? AND directory_id=?",
      bind: [libraryId, contentHash, sourceId],
    });
    if (db.changes() === 0) throw new Error("The book is not present in this folder");
    enqueueSyncMutation(
      db,
      libraryId,
      "placement",
      contentHash,
      { present: false, origin_folder_id: null },
      changedAt,
      sourceId,
    );
    const remaining = Number(
      db.exec({
        sql: "SELECT count(*) FROM book_placements WHERE library_id=? AND content_hash=?",
        bind: [libraryId, contentHash],
        rowMode: 0,
        returnValue: "resultRows",
      })[0],
    );
    const orphaned = remaining === 0;
    if (orphaned) {
      db.exec({
        sql: `INSERT INTO web_trash_books (library_id,content_hash,title,format,deleted_at)
              SELECT library_id,content_hash,title,lower(CASE WHEN instr(file_name,'.')>0 THEN substr(file_name,instr(file_name,'.')+1) ELSE '' END),?
              FROM books WHERE library_id=? AND content_hash=? ON CONFLICT (library_id,content_hash) DO NOTHING`,
        bind: [changedAt, libraryId, contentHash],
      });
      enqueueSyncMutation(
        db,
        libraryId,
        "book_lifecycle",
        contentHash,
        { deleted: { origin_folder_id: null } },
        changedAt,
      );
    }
    db.exec("COMMIT");
    return orphaned;
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export async function getWebChildDirectories(
  libraryId: string,
  parentId: string,
): Promise<Dir[]> {
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
        AND NOT EXISTS (SELECT 1 FROM web_trash_folders trash WHERE trash.library_id=child.library_id AND trash.directory_id=child.directory_id)
      GROUP BY child.directory_id, child.name
      ORDER BY child.name COLLATE NOCASE, child.directory_id
    `,
    bind: [libraryId, parentId, libraryId, libraryId, parentId],
    rowMode: "object",
    returnValue: "resultRows",
  });
  return rows.map((row) => ({
    id: String(row.id),
    name: String(row.name),
    book_count: Number(row.book_count),
    downloaded_book_count: Number(row.book_count),
  }));
}

export async function getWebLibraryFolder(
  libraryId: string,
  directoryId: string,
  fileType: LibraryFileTypeFilter = "all",
): Promise<LibraryHierarchyView> {
  await requireDirectory(libraryId, directoryId);
  const matching = await matchingFileTypeHashes(libraryId, fileType);
  let children = await getDirectoryChildren(libraryId, directoryId);
  if (matching !== null) {
    children = (
      await Promise.all(
        children.map(async (child) => {
          const count = retainMatchingBooks(
            await listBooksInDirectoryTree(libraryId, child.id),
            matching,
          ).length;
          return { ...child, book_count: count, downloaded_book_count: count };
        }),
      )
    ).filter((child) => child.book_count > 0);
  }
  return {
    children,
    books: retainMatchingBooks(await listBooks(libraryId, directoryId), matching),
  };
}

export async function getWebDirectoryPath(
  libraryId: string,
  directoryId: string,
): Promise<Dir[]> {
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
    bind: [libraryId, directoryId, libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  });
  return rows.map(([id, name]) => ({
    id: String(id),
    name: String(name),
    book_count: 0,
    downloaded_book_count: 0,
  }));
}

async function migrateWebDdcPaths(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
): Promise<Map<string, string>> {
  const oldPaths = db
    .exec({
      sql: "SELECT DISTINCT subject_path FROM book_subject_paths WHERE library_id=? AND (subject_path LIKE 'LCC / DDC / %' OR subject_path LIKE 'DDC / %')",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String);
  const replacements = new Map<string, string>();
  await Promise.all(
    oldPaths.map(async (oldPath) => {
      const notation = oldPath.split(" / ").at(-1) ?? "";
      const path = (await unifiedSubjectPaths("ddc", notation))[0];
      if (path && path !== oldPath) replacements.set(oldPath, path);
    }),
  );
  if (!replacements.size) return replacements;
  db.exec("BEGIN IMMEDIATE");
  try {
    for (const [oldPath, path] of replacements) {
      db.exec({
        sql: `INSERT OR IGNORE INTO book_subject_paths(library_id,content_hash,subject_path)
              SELECT library_id,content_hash,? FROM book_subject_paths WHERE library_id=? AND subject_path=?`,
        bind: [path, libraryId, oldPath],
      });
      db.exec({
        sql: "DELETE FROM book_subject_paths WHERE library_id=? AND subject_path=?",
        bind: [libraryId, oldPath],
      });
    }
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  return replacements;
}

async function migrateWebLccPaths(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
): Promise<Map<string, string>> {
  const oldPaths = db
    .exec({
      sql: `SELECT DISTINCT subject_path FROM book_subject_paths
          WHERE library_id=? AND subject_path LIKE 'LCC / %'
            AND subject_path NOT LIKE 'LCC / LCSH / %'
            AND subject_path NOT LIKE 'LCC / LCGFT / %'
            AND subject_path NOT LIKE 'LCC / DDC / %'`,
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String);
  const replacements = new Map<string, string>();
  await Promise.all(
    oldPaths.map(async (oldPath) => {
      const notation = oldPath.split(" / ").at(-1) ?? "";
      const path = (await unifiedSubjectPaths("lcc", notation))[0];
      if (path && path !== oldPath) replacements.set(oldPath, path);
    }),
  );
  if (!replacements.size) return replacements;
  db.exec("BEGIN IMMEDIATE");
  try {
    for (const [oldPath, path] of replacements) {
      db.exec({
        sql: `INSERT OR IGNORE INTO book_subject_paths(library_id,content_hash,subject_path)
              SELECT library_id,content_hash,? FROM book_subject_paths WHERE library_id=? AND subject_path=?`,
        bind: [path, libraryId, oldPath],
      });
      db.exec({
        sql: "DELETE FROM book_subject_paths WHERE library_id=? AND subject_path=?",
        bind: [libraryId, oldPath],
      });
    }
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  return replacements;
}

export async function getWebLibrarySubjects(
  libraryId: string,
  subjectPath: string | null,
  fileType: LibraryFileTypeFilter = "all",
): Promise<LibraryHierarchyView> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const ddcReplacements = await migrateWebDdcPaths(db, libraryId);
  const lccReplacements = await migrateWebLccPaths(db, libraryId);
  subjectPath =
    subjectPath === null
      ? null
      : (ddcReplacements.get(subjectPath) ??
        lccReplacements.get(subjectPath) ??
        subjectPath);
  const matching = await matchingFileTypeHashes(libraryId, fileType);
  const books = retainMatchingBooks(await listBooks(libraryId), matching);
  const storedSubjectRows = db.exec({
    sql: "SELECT content_hash,subject_path FROM book_subject_paths WHERE library_id=?",
    bind: [libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, string]>;
  const hasAnySubject = new Set(storedSubjectRows.map(([contentHash]) => contentHash));
  for (const contentHash of db
    .exec({
      sql: "SELECT DISTINCT content_hash FROM web_book_external_entities WHERE library_id=?",
      bind: [libraryId],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String))
    hasAnySubject.add(contentHash);
  const otherSystems = new Set(
    storedSubjectRows
      .filter(([, path]) => !isUnifiedSubjectPath(path))
      .map(([contentHash]) => contentHash),
  );
  if (subjectPath === "__uncategorized__")
    return {
      children: [],
      books: books.filter((book) => !hasAnySubject.has(book.content_hash)),
    };
  if (subjectPath === "__other_subject_systems__")
    return {
      children: [],
      books: books.filter((book) => otherSystems.has(book.content_hash)),
    };
  if (subjectPath?.startsWith("__places__")) {
    const current = entityIdFromPath(subjectPath);
    if (current) {
      const hashes = new Set(
        db
          .exec({
            sql: "SELECT content_hash FROM web_book_external_entities WHERE library_id=? AND kind='place' AND wikidata_id=?",
            bind: [libraryId, current],
            rowMode: 0,
            returnValue: "resultRows",
          })
          .map(String),
      );
      return {
        children: [],
        books: books.filter((book) => hashes.has(book.content_hash)),
      };
    }
    const places = db.exec({
      sql: `SELECT entity.wikidata_id,entity.label,COUNT(DISTINCT reference.content_hash)
            FROM web_external_entities entity JOIN web_book_external_entities reference
              ON reference.library_id=entity.library_id AND reference.wikidata_id=entity.wikidata_id AND reference.kind='place'
            WHERE entity.library_id=? AND entity.kind='place' GROUP BY entity.wikidata_id,entity.label ORDER BY LOWER(entity.label),entity.wikidata_id`,
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    }) as Array<[string, string, number]>;
    return {
      children: places
        .map(([id, name, count]) => {
          const filteredCount =
            matching === null
              ? Number(count)
              : books.filter(
                  (book) =>
                    db.exec({
                      sql: "SELECT 1 FROM web_book_external_entities WHERE library_id=? AND content_hash=? AND kind='place' AND wikidata_id=? LIMIT 1",
                      bind: [libraryId, book.content_hash, id],
                      rowMode: 0,
                      returnValue: "resultRows",
                    }).length > 0,
                ).length;
          return {
            id: `__places__/${id}`,
            name,
            book_count: filteredCount,
            downloaded_book_count: filteredCount,
          };
        })
        .filter((child) => child.book_count > 0),
      books: [],
    };
  }
  if (subjectPath?.startsWith("__people__")) {
    const current = entityIdFromPath(subjectPath);
    if (current) {
      const hashes = new Set(
        db
          .exec({
            sql: "SELECT content_hash FROM web_book_external_entities WHERE library_id=? AND kind='person' AND wikidata_id=?",
            bind: [libraryId, current],
            rowMode: 0,
            returnValue: "resultRows",
          })
          .map(String),
      );
      return {
        children: [],
        books: books.filter((book) => hashes.has(book.content_hash)),
      };
    }
    const people = db.exec({
      sql: `SELECT entity.wikidata_id,entity.label,COUNT(DISTINCT reference.content_hash)
            FROM web_external_entities entity JOIN web_book_external_entities reference
              ON reference.library_id=entity.library_id AND reference.wikidata_id=entity.wikidata_id AND reference.kind='person'
            WHERE entity.library_id=? AND entity.kind='person' GROUP BY entity.wikidata_id,entity.label ORDER BY LOWER(entity.label),entity.wikidata_id`,
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    }) as Array<[string, string, number]>;
    return {
      children: people
        .map(([id, name, count]) => {
          const filteredCount =
            matching === null
              ? Number(count)
              : books.filter(
                  (book) =>
                    db.exec({
                      sql: "SELECT 1 FROM web_book_external_entities WHERE library_id=? AND content_hash=? AND kind='person' AND wikidata_id=? LIMIT 1",
                      bind: [libraryId, book.content_hash, id],
                      rowMode: 0,
                      returnValue: "resultRows",
                    }).length > 0,
                ).length;
          return {
            id: `__people__/${id}`,
            name,
            book_count: filteredCount,
            downloaded_book_count: filteredCount,
          };
        })
        .filter((child) => child.book_count > 0),
      books: [],
    };
  }

  const parent = subjectPath ?? "";
  const paths = db
    .exec({
      sql: "SELECT DISTINCT subject_path FROM book_subject_paths WHERE library_id=? AND (?='' OR subject_path=? OR subject_path LIKE ?)",
      bind: [libraryId, parent, parent, `${parent} / %`],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String)
    .filter(isUnifiedSubjectPath);
  const booksInSubject = (id: string, recursive: boolean) => {
    const hashes = new Set(
      storedSubjectRows
        .filter(
          ([, path]) =>
            isUnifiedSubjectPath(path) &&
            (path === id || (recursive && path.startsWith(`${id} / `))),
        )
        .map(([contentHash]) => contentHash),
    );
    return books.filter((book) => hashes.has(book.content_hash));
  };
  const immediateChildren = (current: string) => {
    const childNames = new Set<string>();
    for (const path of paths) {
      if (current && !path.startsWith(`${current} / `)) continue;
      const remainder = current ? path.slice(current.length + 3) : path;
      const name = remainder.split(" / ")[0];
      if (name && path !== current) childNames.add(name);
    }
    return [...childNames]
      .map((name) => {
        const id = current ? `${current} / ${name}` : name;
        const count = booksInSubject(id, true).length;
        return { id, name, book_count: count, downloaded_book_count: count };
      })
      .filter((child) => child.book_count > 0)
      .sort((left, right) => left.name.localeCompare(right.name));
  };

  const children: LibraryHierarchyView["children"] = [];
  const hoistedBooks: BookCardRow[] = [];
  for (const child of immediateChildren(parent)) {
    const descendants = booksInSubject(child.id, true);
    if (descendants.length === 1) {
      hoistedBooks.push(descendants[0]);
      continue;
    }
    let display = child;
    while (booksInSubject(display.id, false).length === 0) {
      const next = immediateChildren(display.id);
      if (next.length !== 1) break;
      [display] = next;
    }
    children.push(display);
  }
  if (subjectPath === null) {
    const entityCounts = db.exec({
      sql: "SELECT kind,COUNT(DISTINCT content_hash) FROM web_book_external_entities WHERE library_id=? GROUP BY kind",
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    }) as Array<["place" | "person", number]>;
    for (const [kind, count] of entityCounts)
      if (count)
        children.push({
          id: kind === "place" ? "__places__" : "__people__",
          name: kind === "place" ? "Places" : "Persons",
          book_count: Number(count),
          downloaded_book_count: Number(count),
        });
    const uncategorized = books.filter(
      (book) => !hasAnySubject.has(book.content_hash),
    ).length;
    if (uncategorized)
      children.push({
        id: "__uncategorized__",
        name: "Uncategorized",
        book_count: uncategorized,
        downloaded_book_count: uncategorized,
      });
  }
  const directBooks = subjectPath === null ? [] : booksInSubject(subjectPath, false);
  const seenBooks = new Set(directBooks.map((book) => book.content_hash));
  for (const book of hoistedBooks) {
    if (seenBooks.has(book.content_hash)) continue;
    seenBooks.add(book.content_hash);
    directBooks.push(book);
  }
  return { children, books: directBooks };
}

export async function getWebLibraryAuthors(
  libraryId: string,
  selectedAuthorId: string | null,
  sort: AuthorSort = "name",
): Promise<LibraryAuthorsView> {
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
            AND NOT EXISTS (SELECT 1 FROM web_trash_books trash WHERE trash.library_id=books.library_id AND trash.content_hash=books.content_hash)
          GROUP BY author_id
          ORDER BY ${sort === "book_count" ? "book_count DESC, " : ""}name COLLATE NOCASE, author_id`,
    bind: [FRONTEND_CONTRACT.browsing_layout.author_row_cover_count, libraryId],
    rowMode: "object",
    returnValue: "resultRows",
  });
  const titles = new Map(
    (
      db.exec({
        sql: "SELECT content_hash,title FROM books WHERE library_id=?",
        bind: [libraryId],
        rowMode: "array",
        returnValue: "resultRows",
      }) as Array<[string, string]>
    ).map(([contentHash, title]) => [String(contentHash), String(title)]),
  );
  const authors: AuthorSummary[] = rows.map((row) => {
    const coverHashes = String(row.cover_hashes ?? "")
      .split(",")
      .filter(Boolean);
    return {
      id: String(row.id),
      name: String(row.name),
      sort_name: String(row.name),
      book_count: Number(row.book_count),
      cover_hashes: coverHashes,
      cover_titles: coverHashes.map(
        (contentHash) => titles.get(contentHash) ?? "Untitled",
      ),
    };
  });
  return {
    authors,
    books:
      selectedAuthorId === null
        ? null
        : await listBooks(libraryId, undefined, selectedAuthorId),
  };
}

function titleFromFileName(fileName: string): string {
  return fileName.replace(/\.[^.]+$/, "").trim() || "Untitled";
}

function bookRow(row: Record<string, unknown>): BookCardRow {
  return {
    content_hash: String(row.content_hash),
    title: String(row.title),
    author: String(row.author ?? ""),
    description: String(row.description ?? ""),
    progress: row.progress === null ? null : Number(row.progress),
    downloaded: Boolean(row.downloaded),
    download_requested: false,
    audiobook: false,
    audiobook_duration_ms: null,
    audiobook_chapter_count: null,
    added_at: Number(row.added_at ?? 0),
  };
}

export async function getWebBookDetail(
  libraryId: string,
  contentHash: string,
): Promise<BookDetail> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `SELECT books.content_hash,books.title,books.author,books.description,books.author_id,books.added_at,
                 reading_positions.progress,(books.storage_key IS NOT NULL OR books.bytes IS NOT NULL) AS downloaded
          FROM books LEFT JOIN reading_positions USING (library_id,content_hash)
          WHERE books.library_id=? AND books.content_hash=?
            AND NOT EXISTS (SELECT 1 FROM web_trash_books trash WHERE trash.library_id=books.library_id AND trash.content_hash=books.content_hash)`,
    bind: [libraryId, contentHash],
    rowMode: "object",
    returnValue: "resultRows",
  }) as Array<Record<string, unknown>>;
  if (!rows.length) throw new Error("The book does not exist");
  const row = rows[0];
  const authorId = typeof row.author_id === "string" ? row.author_id : null;
  const sourceDirectoryId = db.exec({
    sql: "SELECT directory_id FROM book_placements WHERE library_id=? AND content_hash=? ORDER BY directory_id LIMIT 1",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  const subjectPaths = db
    .exec({
      sql: "SELECT subject_path FROM book_subject_paths WHERE library_id=? AND content_hash=? ORDER BY subject_path COLLATE NOCASE,subject_path",
      bind: [libraryId, contentHash],
      rowMode: 0,
      returnValue: "resultRows",
    })
    .map(String);
  let cachedToc = db.exec({
    sql: "SELECT toc_json FROM book_toc_cache WHERE library_id=? AND content_hash=?",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  if (cachedToc === undefined) {
    try {
      const fileName = await getWebBookFileName(libraryId, contentHash);
      if (/\.epub$/i.test(fileName)) {
        const source = await openWebBook(libraryId, contentHash);
        const blob =
          typeof source === "string"
            ? await fetch(source).then((response) => {
                if (!response.ok)
                  throw new Error(`Could not fetch publication (${response.status})`);
                return response.blob();
              })
            : source;
        const metadata = await inspectPublication(
          new File([blob], fileName, { type: "application/epub+zip" }),
          fileName,
          new Uint8Array(await blob.arrayBuffer()),
        );
        cachedToc = JSON.stringify(metadata.toc);
        db.exec({
          sql: "INSERT OR REPLACE INTO book_toc_cache (library_id,content_hash,toc_json) VALUES (?,?,?)",
          bind: [libraryId, contentHash, cachedToc],
        });
      }
    } catch {
      // Detail metadata remains usable if a remote-only TOC cannot be fetched.
    }
  }
  const related =
    authorId === null
      ? []
      : (await listBooks(libraryId, undefined, authorId))
          .filter((candidate) => candidate.content_hash !== contentHash)
          .sort(
            (left, right) =>
              left.title.localeCompare(right.title, undefined, {
                sensitivity: "base",
                numeric: true,
              }) || left.content_hash.localeCompare(right.content_hash),
          )
          .slice(0, 12);
  return {
    book: bookRow(row),
    source_directory_id:
      sourceDirectoryId === undefined ? null : String(sourceDirectoryId),
    authors: authorId
      ? [{ id: authorId, name: String(row.author || "Unknown author") }]
      : [],
    subject_paths: subjectPaths,
    toc: cachedToc === undefined ? [] : JSON.parse(String(cachedToc)),
    related,
  };
}

async function listBooks(
  libraryId: string,
  directoryId?: string,
  authorId?: string,
): Promise<BookCardRow[]> {
  const { db } = await getWebDatabase();
  const placementJoin =
    directoryId === undefined
      ? ""
      : "JOIN book_placements placement USING (library_id, content_hash)";
  const filters = [
    "books.library_id = ?",
    "NOT EXISTS (SELECT 1 FROM web_trash_books trash WHERE trash.library_id=books.library_id AND trash.content_hash=books.content_hash)",
  ];
  const bind: string[] = [libraryId];
  if (directoryId !== undefined) {
    filters.push("placement.directory_id = ?");
    bind.push(directoryId);
  }
  if (authorId !== undefined) {
    filters.push("books.author_id = ?");
    bind.push(authorId);
  }
  const rows = db.exec({
    sql: `
      SELECT books.content_hash, books.title, books.author, books.description, books.added_at, reading_positions.progress,
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

async function listBooksInDirectoryTree(
  libraryId: string,
  directoryId: string,
): Promise<BookCardRow[]> {
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `
      WITH RECURSIVE tree(directory_id) AS (
        SELECT ?
        UNION ALL
        SELECT child.directory_id FROM tree
        JOIN directories child ON child.library_id = ? AND child.parent_id = tree.directory_id
      )
      SELECT DISTINCT books.content_hash, books.title, books.author, books.description, books.added_at, reading_positions.progress,
             (books.storage_key IS NOT NULL OR books.bytes IS NOT NULL) AS downloaded
      FROM tree
      JOIN book_placements placement ON placement.library_id = ? AND placement.directory_id = tree.directory_id
      JOIN books USING (library_id, content_hash)
      LEFT JOIN reading_positions USING (library_id, content_hash)
      WHERE NOT EXISTS (SELECT 1 FROM web_trash_books trash WHERE trash.library_id=books.library_id AND trash.content_hash=books.content_hash)
      ORDER BY books.added_at DESC, books.content_hash
    `,
    bind: [directoryId, libraryId, libraryId],
    rowMode: "object",
    returnValue: "resultRows",
  });
  return rows.map((row) => bookRow(row));
}

export async function searchWebLibrary(
  libraryId: string,
  query: string,
  limit: number,
  directoryId?: string,
  fileType: LibraryFileTypeFilter = "all",
): Promise<LibrarySearchResult> {
  await requireLibrary(libraryId);
  const books =
    directoryId === undefined
      ? await listBooks(libraryId)
      : await listBooksInDirectoryTree(libraryId, directoryId);
  return searchBooks(
    retainMatchingBooks(books, await matchingFileTypeHashes(libraryId, fileType)),
    query,
    limit,
  );
}

async function searchBooks(
  books: BookCardRow[],
  query: string,
  limit: number,
): Promise<LibrarySearchResult> {
  const normalizedQuery = await normalizeSearchText(query);
  if (!normalizedQuery) return { title_matches: [], author_matches: [] };
  if (!Number.isSafeInteger(limit) || limit < 1)
    throw new Error("search limit must be a positive integer");
  const normalizedBooks = await Promise.all(
    books.map(async (book) => ({
      book,
      title: await normalizeSearchText(book.title),
      author: await normalizeSearchText(book.author),
    })),
  );
  normalizedBooks.sort(
    (left, right) =>
      left.title.localeCompare(right.title) ||
      left.book.content_hash.localeCompare(right.book.content_hash),
  );
  const titleMatches = normalizedBooks
    .filter(({ title }) => title.includes(normalizedQuery))
    .map(({ book }) => book);
  const titleIds = new Set(titleMatches.map(({ content_hash }) => content_hash));
  const authorMatches = normalizedBooks
    .filter(
      ({ book, author }) =>
        !titleIds.has(book.content_hash) && author.includes(normalizedQuery),
    )
    .map(({ book }) => book);
  return {
    title_matches: titleMatches.slice(0, limit),
    author_matches: authorMatches.slice(0, Math.max(0, limit - titleMatches.length)),
  };
}

export async function searchWebLibrarySubject(
  libraryId: string,
  subjectPath: string | null,
  query: string,
  limit: number,
  fileType: LibraryFileTypeFilter = "all",
): Promise<LibrarySearchResult> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  let rows: unknown[];
  if (
    subjectPath === "__uncategorized__" ||
    subjectPath === "__other_subject_systems__"
  ) {
    const subjectRows = db.exec({
      sql: "SELECT content_hash,subject_path FROM book_subject_paths WHERE library_id=?",
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    }) as Array<[string, string]>;
    if (subjectPath === "__other_subject_systems__") {
      rows = [
        ...new Set(
          subjectRows
            .filter(([, path]) => !isUnifiedSubjectPath(path))
            .map(([contentHash]) => contentHash),
        ),
      ];
    } else {
      const hasAnySubject = new Set(subjectRows.map(([contentHash]) => contentHash));
      for (const contentHash of db
        .exec({
          sql: "SELECT DISTINCT content_hash FROM web_book_external_entities WHERE library_id=?",
          bind: [libraryId],
          rowMode: 0,
          returnValue: "resultRows",
        })
        .map(String))
        hasAnySubject.add(contentHash);
      rows = db
        .exec({
          sql: "SELECT content_hash FROM books WHERE library_id=?",
          bind: [libraryId],
          rowMode: 0,
          returnValue: "resultRows",
        })
        .map(String)
        .filter((contentHash) => !hasAnySubject.has(contentHash));
    }
  } else if (subjectPath?.startsWith("__places__")) {
    const current = entityIdFromPath(subjectPath);
    rows = current
      ? db.exec({
          sql: `SELECT DISTINCT content_hash FROM web_book_external_entities
            WHERE library_id=? AND kind='place' AND wikidata_id=?`,
          bind: [libraryId, current],
          rowMode: 0,
          returnValue: "resultRows",
        })
      : db.exec({
          sql: "SELECT DISTINCT content_hash FROM web_book_external_entities WHERE library_id=? AND kind='place'",
          bind: [libraryId],
          rowMode: 0,
          returnValue: "resultRows",
        });
  } else if (subjectPath?.startsWith("__people__")) {
    const current = entityIdFromPath(subjectPath);
    rows = db.exec({
      sql: `SELECT DISTINCT content_hash FROM web_book_external_entities
            WHERE library_id=? AND kind='person' AND (? IS NULL OR wikidata_id=?)`,
      bind: [libraryId, current, current],
      rowMode: 0,
      returnValue: "resultRows",
    });
  } else
    rows =
      subjectPath === null
        ? []
        : db.exec({
            sql: "SELECT DISTINCT content_hash FROM book_subject_paths WHERE library_id=? AND (subject_path=? OR subject_path LIKE ?)",
            bind: [libraryId, subjectPath, `${subjectPath} / %`],
            rowMode: 0,
            returnValue: "resultRows",
          });
  const allowed = new Set(rows.map(String));
  const books = retainMatchingBooks(
    await listBooks(libraryId),
    await matchingFileTypeHashes(libraryId, fileType),
  );
  return searchBooks(
    subjectPath === null ? books : books.filter((book) => allowed.has(book.content_hash)),
    query,
    limit,
  );
}

export async function importWebBook(
  destination: BookImportDestination,
  file: File,
  fileName: string,
  catalogRequest?: CatalogImportRequest,
  renderedCover?: Uint8Array | null,
): Promise<string> {
  await requireLibrary(destination.library_id);
  await requireDirectory(destination.library_id, destination.directory_id);
  const bytes = new Uint8Array(await file.arrayBuffer());
  const metadata = await inspectPublication(file, fileName, bytes);
  if (metadata.subject_codes.length) {
    await hydrateWebTaxonomy(metadata.subject_codes).catch((reason) =>
      console.warn(
        "Continuing publication import without missing taxonomy detail",
        reason,
      ),
    );
    const hydratedSubjectPaths = await Promise.all(
      metadata.subject_codes.map((classification) =>
        unifiedSubjectPaths(classification.system, classification.code),
      ),
    );
    metadata.subject_paths = [
      ...new Set([...metadata.subject_paths, ...hydratedSubjectPaths.flat()]),
    ].sort();
  }
  if (renderedCover?.length) metadata.cover = renderedCover;
  const author = metadata.author ?? "";
  const authorKey = author ? await normalizeSearchText(author) : "";
  const contentHash = await hashPublication(bytes);
  const format = /\.pdf$/i.test(fileName) ? "Pdf" : "Epub";
  const { db } = await getWebDatabase();
  const previousStorageKey = db.exec({
    sql: "SELECT storage_key FROM books WHERE library_id=? AND content_hash=?",
    bind: [destination.library_id, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  const stored = await storePublication(destination.library_id, contentHash, bytes);
  try {
    const existingAuthor = authorKey
      ? db.exec({
          sql: "SELECT author_id FROM books WHERE library_id = ? AND author_key = ? AND author_id IS NOT NULL LIMIT 1",
          bind: [destination.library_id, authorKey],
          rowMode: 0,
          returnValue: "resultRows",
        })
      : [];
    const authorId = authorKey ? String(existingAuthor[0] ?? crypto.randomUUID()) : null;
    const changedAt = Date.now();
    db.exec("BEGIN IMMEDIATE");
    db.exec({
      sql: `
        INSERT INTO books (library_id, content_hash, file_name, title, author, author_id, author_key, description, cover_bytes, storage_key, bytes, added_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ON CONFLICT (library_id, content_hash) DO UPDATE SET
          file_name = excluded.file_name,
          title = excluded.title,
          author = excluded.author,
          author_id = excluded.author_id,
          author_key = excluded.author_key,
          description = excluded.description,
          cover_bytes = excluded.cover_bytes,
          storage_key = excluded.storage_key,
          bytes = excluded.bytes
      `,
      bind: [
        destination.library_id,
        contentHash,
        fileName,
        metadata.title ?? titleFromFileName(fileName),
        author,
        authorId,
        authorKey,
        metadata.description ?? "",
        metadata.cover,
        stored.storageKey,
        stored.fallbackBytes,
        changedAt,
      ],
    });
    db.exec({
      sql: "DELETE FROM book_subject_paths WHERE library_id=? AND content_hash=?",
      bind: [destination.library_id, contentHash],
    });
    for (const subjectPath of metadata.subject_paths)
      db.exec({
        sql: "INSERT INTO book_subject_paths (library_id,content_hash,subject_path) VALUES (?,?,?)",
        bind: [destination.library_id, contentHash, subjectPath],
      });
    db.exec({
      sql: `INSERT INTO book_toc_cache (library_id,content_hash,toc_json) VALUES (?,?,?)
            ON CONFLICT (library_id,content_hash) DO UPDATE SET toc_json=excluded.toc_json`,
      bind: [destination.library_id, contentHash, JSON.stringify(metadata.toc)],
    });
    db.exec({
      sql: "DELETE FROM web_book_identifiers WHERE library_id=? AND content_hash=? AND scheme='isbn'",
      bind: [destination.library_id, contentHash],
    });
    for (const isbn of metadata.isbns)
      db.exec({
        sql: "INSERT INTO web_book_identifiers (library_id,content_hash,scheme,value) VALUES (?,?,'isbn',?)",
        bind: [destination.library_id, contentHash, isbn],
      });
    db.exec({
      sql: `INSERT INTO book_placements (library_id, content_hash, directory_id, file_name, added_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT (library_id, content_hash, directory_id) DO UPDATE SET file_name = excluded.file_name`,
      bind: [
        destination.library_id,
        contentHash,
        destination.directory_id,
        fileName,
        changedAt,
      ],
    });
    const synchronizedMetadata = {
      title: metadata.title ?? titleFromFileName(fileName),
      contributors: author
        ? [
            {
              contributor_id: authorId,
              name: author,
              sort_name: null,
              role: [97, 117, 116],
            },
          ]
        : [],
      description: metadata.description ?? "",
      publication: {
        subjects: [
          ...metadata.subjects.map((name) => ({
            id: null,
            name,
            authority: null,
            code: null,
          })),
          ...metadata.subject_codes.map((classification) => ({
            id: null,
            name: classification.code,
            authority: classification.system,
            code: classification.code,
          })),
        ],
        subject_paths: metadata.subject_paths,
      },
    };
    enqueueSyncMutation(
      db,
      destination.library_id,
      "book_lifecycle",
      contentHash,
      {
        present: { metadata: synchronizedMetadata, added_at: changedAt, format },
      },
      changedAt,
    );
    enqueueSyncMutation(
      db,
      destination.library_id,
      "metadata",
      contentHash,
      synchronizedMetadata,
      changedAt,
    );
    enqueueSyncMutation(
      db,
      destination.library_id,
      "placement",
      contentHash,
      {
        present: true,
        origin_folder_id: null,
      },
      changedAt,
      destination.directory_id,
    );
    if (catalogRequest)
      recordWebCatalogPublicationInTransaction(
        db,
        destination.library_id,
        contentHash,
        catalogRequest,
        changedAt,
      );
    db.exec("COMMIT");
  } catch (reason) {
    try {
      db.exec("ROLLBACK");
    } catch {
      /* transaction may not have started */
    }
    if (stored.storageKey && stored.storageKey !== previousStorageKey) {
      await removePublication(stored.storageKey).catch(() => undefined);
    }
    throw reason;
  }
  return contentHash;
}

function recordWebCatalogPublicationInTransaction(
  db: Awaited<ReturnType<typeof getWebDatabase>>["db"],
  libraryId: string,
  contentHash: string,
  request: CatalogImportRequest,
  changedAt: number,
): void {
  const workId = request.publication.work_id ?? "";
  const publicationKey = `${new TextEncoder().encode(request.publication.edition_id).length}:${request.publication.edition_id}${workId}`;
  db.exec({
    sql: `INSERT INTO web_catalog_publications (library_id,source_id,edition_id,work_id,content_hash)
          VALUES (?,?,?,?,?)
          ON CONFLICT (library_id,source_id,edition_id,work_id) DO UPDATE SET content_hash=excluded.content_hash`,
    bind: [
      libraryId,
      request.source_id,
      request.publication.edition_id,
      workId,
      contentHash,
    ],
  });
  enqueueSyncMutation(
    db,
    libraryId,
    "catalog_publication",
    request.source_id,
    {
      publication: request.publication,
      content_hash: contentHash,
    },
    changedAt,
    publicationKey,
  );
}

export async function recordWebCatalogPublication(
  libraryId: string,
  contentHash: string,
  request: CatalogImportRequest,
): Promise<void> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  db.exec("BEGIN IMMEDIATE");
  try {
    recordWebCatalogPublicationInTransaction(
      db,
      libraryId,
      contentHash,
      request,
      Date.now(),
    );
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export async function getOwnedWebCatalogPublications(
  libraryId: string,
): Promise<CatalogPublicationKey[]> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  return db
    .exec({
      sql: "SELECT source_id,edition_id,work_id FROM web_catalog_publications WHERE library_id=? ORDER BY source_id,edition_id,work_id",
      bind: [libraryId],
      rowMode: "array",
      returnValue: "resultRows",
    })
    .map(([sourceId, editionId, workId]) => ({
      source_id: String(sourceId),
      publication: { edition_id: String(editionId), work_id: String(workId) || null },
    }));
}

function canonicalSubjectSystems(subjectPath: string): string[] {
  const authority = subjectPath.match(/^LCC \/ (LCSH|LCGFT|LCC|DDC)(?: \/|$)/)?.[1];
  if (authority) return [authority.toLocaleLowerCase()];
  // External systems intentionally share the visible LCC root in the regular
  // subject hierarchy. Native storage can resolve its backing nodes directly;
  // the browser projection retains paths only, so this root expands to the
  // same bounded set of canonical namespaces.
  return subjectPath === "LCC" ? ["lcsh", "lcgft", "lcc", "ddc"] : ["bisac"];
}

export async function getWebCatalogRecommendationSubjects(
  libraryId: string,
  selectedPath: string | null,
): Promise<CatalogSubjectPreference[]> {
  if (
    selectedPath === "__uncategorized__" ||
    selectedPath === "__other_subject_systems__" ||
    selectedPath?.startsWith("__places__") ||
    selectedPath?.startsWith("__people__")
  )
    return [];
  if (selectedPath)
    return canonicalSubjectSystems(selectedPath).map((system_id) => ({
      system_id,
      subject_path: selectedPath,
      weight: 1,
    }));
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: `SELECT subject_path, COUNT(DISTINCT content_hash) AS weight
          FROM book_subject_paths
          WHERE library_id=?
          GROUP BY subject_path
          ORDER BY weight DESC, subject_path
          LIMIT 64`,
    bind: [libraryId],
    rowMode: "array",
    returnValue: "resultRows",
  }) as Array<[string, number]>;
  return rows
    .flatMap(([subject_path, weight]) =>
      canonicalSubjectSystems(subject_path).map((system_id) => ({
        system_id,
        subject_path,
        weight,
      })),
    )
    .slice(0, 64);
}

export async function getWebAuthorName(
  libraryId: string,
  authorId: string,
): Promise<string> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const value = db.exec({
    sql: "SELECT author FROM books WHERE library_id=? AND author_id=? AND author<>'' LIMIT 1",
    bind: [libraryId, authorId],
    rowMode: 0,
    returnValue: "resultRows",
  })[0];
  if (value === undefined) throw new Error("The author does not exist in this library");
  return String(value);
}

export async function getWebBookThumbnail(
  libraryId: string,
  contentHash: string,
): Promise<Uint8Array> {
  await requireLibrary(libraryId);
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

export async function openWebBook(
  libraryId: string,
  contentHash: string,
): Promise<Blob | string> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT storage_key, bytes, file_name FROM books WHERE library_id = ? AND content_hash = ?",
    bind: [libraryId, contentHash],
    rowMode: "array",
    returnValue: "resultRows",
  });
  if (!rows.length) throw new Error("The book does not exist");
  const storageKey = rows[0][0];
  if (typeof storageKey === "string") return readPublication(storageKey);
  const bytes = rows[0][1];
  if (bytes instanceof Uint8Array)
    return new Blob([bytes.slice().buffer], {
      type: /\.pdf$/i.test(String(rows[0][2]))
        ? "application/pdf"
        : "application/epub+zip",
    });
  return `/api/libraries/${encodeURIComponent(libraryId)}/blobs/${encodeURIComponent(contentHash)}`;
}

export async function getWebBookFileName(
  libraryId: string,
  contentHash: string,
): Promise<string> {
  await requireLibrary(libraryId);
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

export async function getWebReadingPosition(
  libraryId: string,
  contentHash: string,
): Promise<string | null> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT cfi FROM reading_positions WHERE library_id = ? AND content_hash = ?",
    bind: [libraryId, contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  });
  return rows.length ? String(rows[0]) : null;
}

function parsePdfReadingPosition(position: string | null): [number, number] | null {
  if (!position) return null;
  const match = /^pdfpage\(([^)]+)\)$/.exec(position);
  if (!match) return null;
  const [pagePart, fullPagePositionPart] = match[1].split(",", 2);
  const page = Number(pagePart);
  if (!Number.isInteger(page) || page < 0) return null;
  const fullPagePosition =
    fullPagePositionPart === undefined ? 0 : Number(fullPagePositionPart);
  if (!Number.isFinite(fullPagePosition) || fullPagePosition < 0) return [page, 0];
  return [page, fullPagePosition];
}

export async function getWebPdfReadingPosition(
  libraryId: string,
  contentHash: string,
): Promise<[number, number] | null> {
  return parsePdfReadingPosition(await getWebReadingPosition(libraryId, contentHash));
}

export async function getWebAudiobookChapters(
  libraryId: string,
  contentHash: string,
): Promise<AudiobookChapter[]> {
  await requireLibrary(libraryId);
  const { db } = await getWebDatabase();
  const rows = db.exec({
    sql: "SELECT chapters_json FROM audiobook_metadata WHERE content_hash = ?",
    bind: [contentHash],
    rowMode: 0,
    returnValue: "resultRows",
  });
  if (!rows.length) return [];
  const chapters = JSON.parse(String(rows[0])) as unknown;
  return Array.isArray(chapters) ? (chapters as AudiobookChapter[]) : [];
}

export async function updateWebReadingPosition(
  libraryId: string,
  contentHash: string,
  cfi: string,
  progress: number | null,
): Promise<void> {
  await requireLibrary(libraryId);
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
    enqueueSyncMutation(
      db,
      libraryId,
      "reading_position",
      contentHash,
      { location: cfi, progress: progress ?? 0 },
      changedAt,
    );
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
}

export async function updateWebPdfReadingPosition(
  libraryId: string,
  contentHash: string,
  page: number,
  fullPagePosition: number | null,
): Promise<void> {
  await updateWebReadingPosition(
    libraryId,
    contentHash,
    `pdfpage(${Math.max(0, Math.floor(page))},${Math.max(0, typeof fullPagePosition === "number" && Number.isFinite(fullPagePosition) ? fullPagePosition : 0)})`,
    null,
  );
}
