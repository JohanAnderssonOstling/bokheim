import { getWebDatabase } from "./database.ts";
import { installTaxonomyOverlay, unifiedHasExactSourceCode, unifiedSubjectPaths } from "./wasm-core.ts";

export type TaxonomySystem = "bisac" | "ddc" | "lcc";
export interface TaxonomyCode { system: TaxonomySystem; code: string }
interface TaxonomyParent { concept_id: string; display_order: number }
interface TaxonomyConcept {
  concept_id: string;
  preferred_label: string;
  parents: TaxonomyParent[];
  source_selectors: Record<string, string[]>;
  source_labels: string[];
}
interface TaxonomyResolution extends TaxonomyCode { matched_concept_ids: string[] }
interface TaxonomyBatchResponse { release_id: string; results: TaxonomyResolution[]; concepts: TaxonomyConcept[] }

const TAXONOMY_ORIGIN = "https://taxonomy.bokheim.se";
const MAX_CODES = 512;
const MAX_CODE_BYTES = 256;
const MAX_REQUEST_BYTES = 64 * 1024;
const MAX_RESPONSE_BYTES = 8 * 1024 * 1024;
const encoder = new TextEncoder();

function key(code: TaxonomyCode): string {
  return `${code.system}\0${code.code}`;
}

function normalizeCodes(codes: Iterable<TaxonomyCode>): TaxonomyCode[] {
  const unique = new Map<string, TaxonomyCode>();
  for (const requested of codes) {
    const code = requested.code.trim().toUpperCase();
    const bytes = encoder.encode(code).length;
    if (!bytes || bytes > MAX_CODE_BYTES) throw new Error(`Taxonomy codes must contain between 1 and ${MAX_CODE_BYTES} bytes`);
    const normalized = { system: requested.system, code };
    unique.set(key(normalized), normalized);
  }
  return [...unique.values()].sort((left, right) => key(left).localeCompare(key(right)));
}

function prepareBatches(codes: TaxonomyCode[]): TaxonomyCode[][] {
  const batches: TaxonomyCode[][] = [];
  let current: TaxonomyCode[] = [];
  for (const code of codes) {
    const candidate = [...current, code];
    if (current.length && (candidate.length > MAX_CODES || encoder.encode(JSON.stringify({ codes: candidate })).length > MAX_REQUEST_BYTES)) {
      batches.push(current);
      current = [code];
    } else current = candidate;
  }
  if (current.length) batches.push(current);
  return batches;
}

function isBatchResponse(value: unknown): value is TaxonomyBatchResponse {
  if (!value || typeof value !== "object") return false;
  const response = value as Record<string, unknown>;
  return typeof response.release_id === "string" && Boolean(response.release_id) && Array.isArray(response.results) && Array.isArray(response.concepts);
}

async function fetchBatch(codes: TaxonomyCode[]): Promise<TaxonomyBatchResponse> {
  const body = JSON.stringify({ codes });
  const response = await fetch(`${TAXONOMY_ORIGIN}/v1/resolve`, {
    method: "POST", headers: { Accept: "application/json", "Content-Type": "application/json" }, body,
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) throw new Error(`Taxonomy server returned HTTP ${response.status}`);
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > MAX_RESPONSE_BYTES) throw new Error("Taxonomy response is too large");
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length > MAX_RESPONSE_BYTES) throw new Error("Taxonomy response is too large");
  const decoded: unknown = JSON.parse(new TextDecoder().decode(bytes));
  if (!isBatchResponse(decoded)) throw new Error("Taxonomy server returned an invalid response");

  const expected = new Set(codes.map(key));
  const actual = new Set<string>();
  for (const result of decoded.results) {
    if (!result || !["bisac", "ddc", "lcc"].includes(result.system) || typeof result.code !== "string" || !Array.isArray(result.matched_concept_ids)) {
      throw new Error("Taxonomy server returned an invalid code resolution");
    }
    actual.add(key({ system: result.system, code: result.code }));
  }
  if (actual.size !== decoded.results.length || actual.size !== expected.size || [...expected].some((requested) => !actual.has(requested))) {
    throw new Error("Taxonomy response did not contain exactly one result for every requested code");
  }
  const conceptIds = new Set(decoded.concepts.map((concept) => concept?.concept_id));
  if (decoded.concepts.some((concept) => typeof concept?.concept_id !== "string") || conceptIds.size !== decoded.concepts.length) {
    throw new Error("Taxonomy response contained invalid or duplicate concepts");
  }
  if (decoded.results.some((result) => result.matched_concept_ids.some((conceptId) => !conceptIds.has(conceptId)))) {
    throw new Error("Taxonomy response omitted a matched concept from its ancestor slice");
  }
  return decoded;
}

async function installPersistedOverlay(db: Awaited<ReturnType<typeof getWebDatabase>>["db"], forceReset = false): Promise<void> {
  const rows = db.exec({ sql: "SELECT concept_json FROM web_taxonomy_concepts ORDER BY concept_id", rowMode: 0, returnValue: "resultRows" });
  if (!rows.length && !forceReset) return;
  await installTaxonomyOverlay(rows.map((json) => JSON.parse(String(json))));
}

async function hydrateWebTaxonomyInner(requested: Iterable<TaxonomyCode>): Promise<void> {
  const { db } = await getWebDatabase();
  await installPersistedOverlay(db);
  const codes = normalizeCodes(requested);
  const missing: TaxonomyCode[] = [];
  for (const code of codes) {
    if (await unifiedHasExactSourceCode(code.system, code.code)) continue;
    const persisted = db.exec({
      sql: "SELECT 1 FROM web_taxonomy_resolutions WHERE system_id=? AND code=?",
      bind: [code.system, code.code], rowMode: 0, returnValue: "resultRows",
    });
    if (!persisted.length) missing.push(code);
  }
  if (!missing.length) return;

  let releaseId: string | undefined;
  const results: TaxonomyResolution[] = [];
  const concepts = new Map<string, TaxonomyConcept>();
  for (const batch of prepareBatches(missing)) {
    const response = await fetchBatch(batch);
    if (releaseId !== undefined && releaseId !== response.release_id) throw new Error("Taxonomy release changed while resolving a multi-batch operation");
    releaseId = response.release_id;
    results.push(...response.results);
    for (const concept of response.concepts) {
      const previous = concepts.get(concept.concept_id);
      if (previous && JSON.stringify(previous) !== JSON.stringify(concept)) throw new Error(`Taxonomy concept ${concept.concept_id} changed between batches`);
      concepts.set(concept.concept_id, concept);
    }
  }
  if (releaseId === undefined) return;

  const hadConceptOverlay = db.exec({ sql: "SELECT 1 FROM web_taxonomy_concepts LIMIT 1", rowMode: 0, returnValue: "resultRows" }).length > 0;
  db.exec("BEGIN IMMEDIATE");
  try {
    const previous = db.exec({ sql: "SELECT server_release_id FROM web_taxonomy_resolutions LIMIT 1", rowMode: 0, returnValue: "resultRows" })[0];
    if (previous !== undefined && String(previous) !== releaseId) {
      db.exec("DELETE FROM web_taxonomy_resolutions");
      db.exec("DELETE FROM web_taxonomy_concepts");
    }
    for (const concept of concepts.values()) db.exec({
      sql: "INSERT INTO web_taxonomy_concepts(concept_id,concept_json) VALUES (?,?) ON CONFLICT(concept_id) DO UPDATE SET concept_json=excluded.concept_json",
      bind: [concept.concept_id, JSON.stringify(concept)],
    });
    for (const result of results) db.exec({
      sql: `INSERT INTO web_taxonomy_resolutions(system_id,code,server_release_id,matched_concept_ids_json) VALUES (?,?,?,?)
            ON CONFLICT(system_id,code) DO UPDATE SET server_release_id=excluded.server_release_id,matched_concept_ids_json=excluded.matched_concept_ids_json`,
      bind: [result.system, result.code, releaseId, JSON.stringify(result.matched_concept_ids)],
    });
    db.exec("COMMIT");
  } catch (reason) {
    db.exec("ROLLBACK");
    throw reason;
  }
  await installPersistedOverlay(db, hadConceptOverlay);
}

let hydrationQueue: Promise<void> = Promise.resolve();
export function hydrateWebTaxonomy(codes: Iterable<TaxonomyCode>): Promise<void> {
  const copy = [...codes];
  const operation = hydrationQueue.then(() => hydrateWebTaxonomyInner(copy));
  hydrationQueue = operation.catch(() => undefined);
  return operation;
}

function publicationFromMutationValue(value: unknown): Record<string, unknown> | null {
  if (!value || typeof value !== "object") return null;
  const root = value as Record<string, unknown>;
  const metadata = root.publication ? root : (root.present as Record<string, unknown> | undefined)?.metadata;
  const publication = metadata && typeof metadata === "object" ? (metadata as Record<string, unknown>).publication : undefined;
  return publication && typeof publication === "object" ? publication as Record<string, unknown> : null;
}

function codesFromPublication(publication: Record<string, unknown>): TaxonomyCode[] {
  const subjects = Array.isArray(publication.subjects) ? publication.subjects : [];
  const codes: TaxonomyCode[] = [];
  for (const value of subjects) {
    if (!value || typeof value !== "object") continue;
    const subject = value as Record<string, unknown>;
    if ((subject.authority === "bisac" || subject.authority === "ddc" || subject.authority === "lcc") && typeof subject.code === "string") {
      codes.push({ system: subject.authority, code: subject.code });
    }
  }
  return codes;
}

export async function hydrateMutationTaxonomy(mutations: Array<{ kind: string; value: unknown }>): Promise<void> {
  const publications = mutations
    .filter((mutation) => mutation.kind === "metadata" || mutation.kind === "book_lifecycle")
    .map((mutation) => publicationFromMutationValue(mutation.value))
    .filter((publication): publication is Record<string, unknown> => publication !== null);
  const codes = publications.flatMap(codesFromPublication);
  await hydrateWebTaxonomy(codes);
  for (const publication of publications) {
    const paths = new Set(Array.isArray(publication.subject_paths) ? publication.subject_paths.filter((path): path is string => typeof path === "string") : []);
    for (const code of codesFromPublication(publication)) {
      for (const path of await unifiedSubjectPaths(code.system, code.code.trim())) paths.add(path);
    }
    publication.subject_paths = [...paths].sort();
  }
}
