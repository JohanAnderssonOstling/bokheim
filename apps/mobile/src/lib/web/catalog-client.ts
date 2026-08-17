import type {
  BookImportDestination,
  CatalogAuthorDiscoveryResult,
  CatalogImportRequest,
  CatalogPage,
  CatalogPageView,
  CatalogRecommendationResponse,
  CatalogSearch,
  CatalogSearchQuery,
  CatalogSource,
  CatalogSubjectBrowseResponse,
  CatalogSubjectBrowseView,
  CatalogWideSearchGroup,
  CatalogWideSearchView,
} from "../bindings.ts";
import { getOwnedWebCatalogPublications, getWebAuthorName, getWebCatalogRecommendationSubjects, importWebBook } from "./library-database.ts";
import { getWebBrowsingPreferences } from "./preferences-database.ts";
import { renderPdfFirstPageJpeg } from "./pdf-thumbnail.ts";
import { normalizeSearchText } from "./wasm-core.ts";
import { BROWSING_PREFERENCES_CONTRACT } from "../types.ts";

const CATALOG_ORIGIN = "https://catalogs.bokheim.se";
const CATALOG_API = `${CATALOG_ORIGIN}/api/web`;
const REQUEST_TIMEOUT_MS = 15_000;

let cachedSources: CatalogSource[] = [];

async function catalogFetch(path: string, init: RequestInit = {}): Promise<Response> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    const response = await fetch(`${CATALOG_API}${path}`, { ...init, signal: controller.signal });
    if (!response.ok) {
      const detail = (await response.text().catch(() => "")).trim();
      throw new Error(detail || `Catalogue server returned HTTP ${response.status}`);
    }
    return response;
  } catch (reason) {
    if (reason instanceof DOMException && reason.name === "AbortError") throw new Error("Catalogue request timed out");
    throw reason;
  } finally {
    clearTimeout(timeout);
  }
}

async function catalogJson<T>(path: string, value?: unknown): Promise<T> {
  const response = await catalogFetch(path, value === undefined ? undefined : {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(value),
  });
  return response.json() as Promise<T>;
}

async function supportedLanguages(): Promise<string[]> {
  return (await getWebBrowsingPreferences()).included_catalog_languages;
}

function sourceMatchesLanguages(source: CatalogSource, languages: string[]): boolean {
  if (!languages.length || source.languages.some((language) => language.toLocaleLowerCase() === "multilingual")) return true;
  const includedLabels = new Set(BROWSING_PREFERENCES_CONTRACT.options.catalog_languages
    .filter((option) => languages.includes(option.value))
    .map((option) => option.label.toLocaleLowerCase()));
  return source.languages.some((language) => includedLabels.has(language.toLocaleLowerCase()));
}

function requestState(languages: string[]) {
  return { supported_languages: languages, epoch: null, known_publications: [] };
}

export function getWebCatalogSources(): CatalogSource[] {
  return [...cachedSources];
}

export async function refreshWebCatalogSources(): Promise<CatalogSource[]> {
  cachedSources = await catalogJson<CatalogSource[]>("/catalogs");
  return [...cachedSources];
}

async function sources(): Promise<CatalogSource[]> {
  return cachedSources.length ? cachedSources : refreshWebCatalogSources();
}

export async function getWebCatalogPage(libraryId: string, sourceId: string, locator: string | null): Promise<CatalogPageView> {
  const languages = await supportedLanguages();
  const page = await catalogJson<CatalogPage>(`/sources/${encodeURIComponent(sourceId)}/page`, {
    locator,
    ...requestState(languages),
  });
  return { page, owned_publications: await getOwnedWebCatalogPublications(libraryId) };
}

export async function getWebCatalogSubjects(libraryId: string, locator: string | null, limit: number): Promise<CatalogSubjectBrowseView> {
  const response = await catalogJson<CatalogSubjectBrowseResponse>("/subjects", {
    locator,
    supported_languages: await supportedLanguages(),
    limit: Math.max(1, Math.min(100, Math.trunc(limit))),
  });
  return { response, owned_publications: await getOwnedWebCatalogPublications(libraryId) };
}

export async function getWebCatalogSubjectRecommendations(libraryId: string, subjectPath: string | null, limit: number): Promise<CatalogRecommendationResponse> {
  const boundedLimit = Math.max(1, Math.min(100, Math.trunc(limit)));
  const response = await catalogJson<CatalogRecommendationResponse>("/recommendations", {
    subjects: await getWebCatalogRecommendationSubjects(libraryId, subjectPath),
    supported_languages: await supportedLanguages(),
    excluded_publications: [],
    limit: Math.min(100, boundedLimit * 3),
  });
  const owned = new Set((await getOwnedWebCatalogPublications(libraryId)).map((key) => `${key.source_id}:${key.publication.edition_id}:${key.publication.work_id ?? ""}`));
  response.recommendations = response.recommendations
    .filter((item) => !owned.has(`${item.source_id}:${item.publication.identity.edition_id}:${item.publication.identity.work_id ?? ""}`))
    .slice(0, boundedLimit);
  return response;
}

async function searchSource(source: CatalogSource, query: string, languages: string[]): Promise<CatalogWideSearchGroup> {
  try {
    const page = await catalogJson<CatalogPage>(`/sources/${encodeURIComponent(source.id)}/search`, {
      query,
      ...requestState(languages),
    });
    return { source, outcome: { kind: "results", page } };
  } catch (reason) {
    return { source, outcome: { kind: "failed", message: reason instanceof Error ? reason.message : String(reason) } };
  }
}

export async function searchWebCatalogs(libraryId: string, query: string): Promise<CatalogWideSearchView> {
  const terms = query.trim();
  if (!terms) throw new Error("Catalogue search cannot be empty");
  const languages = await supportedLanguages();
  const groups = await Promise.all((await sources()).filter((source) => sourceMatchesLanguages(source, languages)).map((source) => searchSource(source, terms, languages)));
  return {
    result: { query: terms, groups },
    owned_publications: await getOwnedWebCatalogPublications(libraryId),
  };
}

export async function searchWebCatalog(
  libraryId: string,
  sourceId: string,
  _search: CatalogSearch,
  query: CatalogSearchQuery,
): Promise<CatalogPageView> {
  const terms = [query.query, query.title, query.author].map((value) => value?.trim()).filter(Boolean).join(" ");
  if (!terms) throw new Error("Catalogue search cannot be empty");
  const page = await catalogJson<CatalogPage>(`/sources/${encodeURIComponent(sourceId)}/search`, {
    query: terms,
    ...requestState(await supportedLanguages()),
  });
  return { page, owned_publications: await getOwnedWebCatalogPublications(libraryId) };
}

export async function getWebCatalogCover(sourceId: string, imageUrl: string): Promise<Uint8Array> {
  const url = new URL(`${CATALOG_ORIGIN}/api/opds/sources/${encodeURIComponent(sourceId)}/cover`);
  url.searchParams.set("image_url", imageUrl);
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
  try {
    const response = await fetch(url, { signal: controller.signal });
    if (!response.ok) throw new Error(`Catalogue cover returned HTTP ${response.status}`);
    if (response.headers.get("content-type")?.split(";", 1)[0].trim().toLowerCase() !== "image/jpeg") {
      throw new Error("Catalogue cover was not a JPEG image");
    }
    return new Uint8Array(await response.arrayBuffer());
  } finally {
    clearTimeout(timeout);
  }
}

function acquisitionFileName(request: CatalogImportRequest): string {
  const extension = request.book_format === "web_pub" ? "webpub" : request.book_format;
  try {
    const candidate = decodeURIComponent(new URL(request.download_url).pathname.split("/").filter(Boolean).at(-1) ?? "");
    if (candidate && !/[\\/\0]/.test(candidate)) return candidate.toLocaleLowerCase().endsWith(`.${extension}`) ? candidate : `${candidate}.${extension}`;
  } catch {
    // The Rust service performs authoritative URL validation.
  }
  return `${request.publication.edition_id.replace(/[^a-zA-Z0-9._-]+/g, "-") || "catalogue-book"}.${extension}`;
}

export async function importWebCatalogBook(destination: BookImportDestination, request: CatalogImportRequest): Promise<void> {
  if (request.book_format !== "epub" && request.book_format !== "pdf") {
    throw new Error("The web library currently accepts EPUB and PDF catalogue downloads only");
  }
  const mime = request.book_format === "pdf" ? "application/pdf" : "application/epub+zip";
  const response = await catalogFetch(`/sources/${encodeURIComponent(request.source_id)}/acquire`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: mime },
    body: JSON.stringify(request),
  });
  const bytes = await response.arrayBuffer();
  const fileName = acquisitionFileName(request);
  const file = new File([bytes], fileName, { type: response.headers.get("content-type") ?? mime });
  const cover = await renderPdfFirstPageJpeg(file, fileName);
  await importWebBook(destination, file, fileName, request, cover);
}

export async function discoverWebCatalogAuthorBooks(libraryId: string, authorId: string): Promise<CatalogAuthorDiscoveryResult> {
  const authorName = await getWebAuthorName(libraryId, authorId);
  const expected = await normalizeSearchText(authorName);
  const view = await searchWebCatalogs(libraryId, authorName);
  const owned = new Set(view.owned_publications.map((key) => `${key.source_id}:${key.publication.edition_id}:${key.publication.work_id ?? ""}`));
  const seen = new Set<string>();
  const matches = [];
  for (const group of view.result.groups) {
    if (group.outcome.kind !== "results") continue;
    const publications = [...group.outcome.page.publications, ...group.outcome.page.groups.flatMap((resultGroup) => resultGroup.publications)];
    for (const publication of publications) {
      const key = `${group.source.id}:${publication.identity.edition_id}:${publication.identity.work_id ?? ""}`;
      if (seen.has(key) || owned.has(key)) continue;
      const matchingNames = await Promise.all(publication.authors.map((author) => normalizeSearchText(author.name)));
      if (!matchingNames.includes(expected)) continue;
      seen.add(key);
      matches.push({ source: group.source, publication, evidence: { kind: "author_name" as const } });
      if (matches.length >= 100) break;
    }
    if (matches.length >= 100) break;
  }
  return { author_name: authorName, matches };
}
