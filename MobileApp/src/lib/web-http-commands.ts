import type { AuthorSummary, BookCardRow, Dir, DownloadState, LibraryAuthorsView, LibraryFolderView, LibraryHomeView } from "./bindings";
import type { commands as nativeCommands } from "./bindings";
import { validateReadingPositionLocally } from "./browser-algorithms";
import { takeBrowserImport } from "./epub-picker";
import { WEB_PREFERENCE_PROFILES_CONTRACT, type WebPreferenceProfile } from "./types";

const configuredBase = (import.meta.env.VITE_BOKHEIM_WEB_API as string | undefined)?.replace(/\/$/, "") ?? "";
const browserSessionRefreshIntervalMs = 45 * 60 * 1000;
const jsonRequestTimeoutMs = 20_000;
const thumbnailRequestTimeoutMs = 20_000;
const bookRequestTimeoutMs = 120_000;
const automaticSyncCommands = new Set([
  "create_library",
  "create_directory",
  "import_opds_book",
  "import_catalog_book",
  "update_reading_position",
  "upsert_annotation",
  "delete_annotation",
  "create_bookmark",
  "delete_bookmark",
  "upsert_book_link",
  "delete_book_link",
]);
let refreshBrowserSessionAfter = 0;
let automaticSyncTimer: ReturnType<typeof setTimeout> | undefined;
let automaticSyncRetryMs = 1000;
const webPreferenceProfile: WebPreferenceProfile = window.matchMedia(`(min-width: ${WEB_PREFERENCE_PROFILES_CONTRACT.desktop_min_width_px}px)`).matches
  ? "desktop"
  : "mobile";

type LegacyBookSummary = {
  id: string;
  title: string;
  author: string;
  read_progress: number;
  last_read: number;
  added_at: number;
  download_state: DownloadState;
};
type LegacyDirectoryContent = { books: LegacyBookSummary[]; dirs: Dir[] };
type LegacyAuthorBooks = { books: LegacyBookSummary[] };

function bookCard(book: LegacyBookSummary): BookCardRow {
  return {
    book_id: book.id,
    title: book.title,
    author: book.author,
    description: "",
    progress: book.read_progress,
    downloaded: book.download_state === "Downloaded",
    download_requested: book.download_state !== "NotDownloaded" && book.download_state !== "Downloaded",
    audiobook: false,
    audiobook_duration_ms: null,
    audiobook_chapter_count: null,
  };
}

async function responseError(response: Response): Promise<Error> {
  const fallback = `${response.status} ${response.statusText}`;
  try {
    const body = await response.json() as { error?: unknown };
    return new Error(typeof body.error === "string" ? body.error : fallback);
  } catch {
    return new Error(fallback);
  }
}

async function jsonRequest<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(`${configuredBase}${path}`, {
      credentials: "include",
      ...init,
      signal: init?.signal ?? AbortSignal.timeout(jsonRequestTimeoutMs),
      headers: { "content-type": "application/json", ...init?.headers },
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === "TimeoutError") throw new Error("The web server request timed out.");
    throw error;
  }
  if (!response.ok) throw await responseError(response);
  return response.json() as Promise<T>;
}

async function rpc<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  await refreshBrowserSessionIfDue();
  const result = await jsonRequest<T>("/api/web/rpc", { method: "POST", body: JSON.stringify({ command, args }) });
  if (automaticSyncCommands.has(command)) scheduleAutomaticSync();
  return result;
}

function cancelAutomaticSync() {
  if (automaticSyncTimer !== undefined) clearTimeout(automaticSyncTimer);
  automaticSyncTimer = undefined;
  automaticSyncRetryMs = 1000;
}

function scheduleAutomaticSync(delayMs = 750) {
  if (automaticSyncTimer !== undefined) clearTimeout(automaticSyncTimer);
  automaticSyncTimer = setTimeout(() => void runAutomaticSync(), delayMs);
}

async function runAutomaticSync() {
  automaticSyncTimer = undefined;
  try {
    await rpc("sync_all_libraries");
    automaticSyncRetryMs = 1000;
  } catch {
    const retryAfter = automaticSyncRetryMs;
    automaticSyncRetryMs = Math.min(automaticSyncRetryMs * 2, 60_000);
    scheduleAutomaticSync(retryAfter);
  }
}

async function binary(path: string, timeoutMs = bookRequestTimeoutMs): Promise<number[]> {
  await refreshBrowserSessionIfDue();
  let response: Response;
  try {
    response = await fetch(`${configuredBase}${path}`, { credentials: "include", signal: AbortSignal.timeout(timeoutMs) });
  } catch (error) {
    if (error instanceof DOMException && error.name === "TimeoutError") throw new Error("The web server request timed out.");
    throw error;
  }
  if (!response.ok) throw await responseError(response);
  return [...new Uint8Array(await response.arrayBuffer())];
}

async function refreshBrowserSession() {
  const status = await jsonRequest("/api/web/auth/refresh", { method: "POST", body: "{}" });
  refreshBrowserSessionAfter = Date.now() + browserSessionRefreshIntervalMs;
  return status;
}

async function refreshBrowserSessionIfDue() {
  if (refreshBrowserSessionAfter !== 0 && Date.now() >= refreshBrowserSessionAfter) {
    await refreshBrowserSession();
  }
}

async function webAccountStatus() {
  const status = await jsonRequest<{ signed_in: boolean } & Record<string, unknown>>("/api/web/auth/session");
  if (!status.signed_in) {
    refreshBrowserSessionAfter = 0;
    cancelAutomaticSync();
    return status;
  }
  return refreshBrowserSession();
}

async function login(email: string, password: string) {
  cancelAutomaticSync();
  const status = await jsonRequest("/api/web/auth/login", { method: "POST", body: JSON.stringify({ email, password }) });
  refreshBrowserSessionAfter = Date.now() + browserSessionRefreshIntervalMs;
  return status;
}

async function getAccountSessions() {
  return jsonRequest("/api/web/auth/sessions");
}

async function revokeAccountSession(sessionId: string) {
  const status = await jsonRequest<{ signed_in: boolean } & Record<string, unknown>>(`/api/web/auth/sessions/${encodeURIComponent(sessionId)}`, { method: "DELETE" });
  if (!status.signed_in) {
    refreshBrowserSessionAfter = 0;
    cancelAutomaticSync();
  }
  return status;
}

async function revokeOtherAccountSessions() {
  return jsonRequest<number>("/api/web/auth/sessions", { method: "DELETE" });
}

async function requestPublicRegistration(email: string, password: string) {
  return jsonRequest<null>("/api/web/auth/register", { method: "POST", body: JSON.stringify({ email, password }) });
}

async function resendVerification(email: string) {
  return jsonRequest<null>("/api/web/auth/resend-verification", { method: "POST", body: JSON.stringify({ email }) });
}

async function verifyEmail(token: string) {
  cancelAutomaticSync();
  const status = await jsonRequest("/api/web/auth/verify-email", { method: "POST", body: JSON.stringify({ token }) });
  refreshBrowserSessionAfter = Date.now() + browserSessionRefreshIntervalMs;
  return status;
}

async function requestPasswordReset(email: string) {
  return jsonRequest<null>("/api/web/auth/request-password-reset", { method: "POST", body: JSON.stringify({ email }) });
}

async function resetPassword(token: string, newPassword: string) {
  return jsonRequest<null>("/api/web/auth/reset-password", { method: "POST", body: JSON.stringify({ token, new_password: newPassword }) });
}

async function logout() {
  cancelAutomaticSync();
  const status = await jsonRequest("/api/web/auth/logout", { method: "POST", body: "{}" });
  refreshBrowserSessionAfter = 0;
  return status;
}

const implementation = {
  initLogging: async () => undefined,
  backendLinkStatus: () => rpc<string>("backend_link_status"),
  getLibraries: () => rpc("get_libraries"),
  getAccountStatus: webAccountStatus,
  getAccountSessions,
  revokeAccountSession,
  revokeOtherAccountSessions,
  login,
  requestPublicRegistration,
  resendVerification,
  verifyEmail,
  requestPasswordReset,
  resetPassword,
  bootstrapRemoteLibraries: () => rpc("bootstrap_remote_libraries"),
  syncAllLibraries: () => rpc("sync_all_libraries"),
  getLibrarySyncStatuses: () => rpc("get_library_sync_statuses"),
  reconcileLibraryCatalogue: () => rpc("reconcile_library_catalogue"),
  getTransferStatuses: () => rpc("get_transfer_statuses"),
  getNetworkStatus: () => rpc("get_network_status"),
  updateNetworkStatus: (status: unknown) => rpc("update_network_status", { status }),
  getTransferPreferences: () => rpc("get_transfer_preferences", { profile: webPreferenceProfile }),
  updateTransferPreference: (patch: unknown) => rpc("update_transfer_preference", { profile: webPreferenceProfile, patch }),
  logout,
  getReaderPreferences: () => rpc("get_reader_preferences", { profile: webPreferenceProfile }),
  updateReaderPreference: (patch: unknown) => rpc("update_reader_preference", { profile: webPreferenceProfile, patch }),
  getBrowsingPreferences: () => rpc("get_browsing_preferences", { profile: webPreferenceProfile }),
  updateBrowsingPreference: (patch: unknown) => rpc("update_browsing_preference", { profile: webPreferenceProfile, patch }),
  createLibrary: (name: string) => rpc("create_library", { name }),
  openLibraryFolder: (_path: string) => Promise.reject(new Error("Opening local library folders is unavailable in the web app")),
  removeLibrary: (libraryId: string) => rpc("remove_library", { libraryId }),
  getLibraryFolder: async (libraryId: string, dirId: string): Promise<LibraryFolderView> => {
    const content = await rpc<LegacyDirectoryContent>("get_dir_contents", { libraryId, dirId });
    return { directories: content.dirs, books: content.books.map(bookCard) };
  },
  getChildDirectories: (libraryId: string, parentId: string) => rpc("get_child_directories", { libraryId, parentId }),
  createDirectory: (libraryId: string, parentId: string, name: string) => rpc("create_directory", { libraryId, parentId, name }),
  importBook: async (destination: unknown, sourcePath: string, fileName: string) => {
    await refreshBrowserSessionIfDue();
    const file = takeBrowserImport(sourcePath);
    if (!file) throw new Error("The selected browser file is no longer available. Choose it again.");
    const form = new FormData();
    form.set("destination", JSON.stringify(destination));
    form.set("file", file, fileName);
    const response = await fetch(`${configuredBase}/api/web/books/import`, { method: "POST", credentials: "include", body: form });
    if (!response.ok) throw await responseError(response);
    await response.json();
    scheduleAutomaticSync();
    return null;
  },
  getOpdsEndpoints: () => rpc("get_opds_endpoints"),
  refreshOpdsEndpoints: () => rpc("refresh_opds_endpoints"),
  getOpdsCatalog: (endpoint: unknown, url: string | null, username: string | null, password: string | null) => rpc("get_opds_catalog", { endpoint, url, username, password }),
  searchOpdsCatalog: (endpoint: unknown, search: unknown, query: string, username: string | null, password: string | null) => rpc("search_opds_catalog", { endpoint, search, query, username, password }),
  getOpdsOwnership: (libraryId: string, endpoint: unknown, publications: unknown[]) => rpc("get_opds_ownership", { libraryId, endpoint, publications }),
  importOpdsBook: (destination: unknown, endpoint: unknown, publication: unknown, acquisition: unknown, username: string | null, password: string | null) => rpc("import_opds_book", { destination, endpoint, publication, acquisition, username, password }),
  getCatalogSources: () => rpc("get_catalog_sources"),
  refreshCatalogSources: () => rpc("refresh_catalog_sources"),
  getCustomCatalogSources: async () => [],
  addCustomCatalog: async () => Promise.reject(new Error("Custom catalogs are device-local and unavailable in the hosted web app")),
  removeCustomCatalog: async () => Promise.reject(new Error("Custom catalogs are device-local and unavailable in the hosted web app")),
  searchCatalogs: (query: string) => rpc("search_catalogs", { query }),
  getCatalogPage: (sourceId: unknown, locator: string | null, username: string | null, password: string | null) => rpc("get_catalog_page", { sourceId, locator, username, password }),
  searchCatalog: (sourceId: unknown, search: unknown, query: unknown, username: string | null, password: string | null) => rpc("search_catalog", { sourceId, search, query, username, password }),
  getCatalogCover: (sourceId: string, imageUrl: string) => binary(`/api/web/catalogs/${encodeURIComponent(sourceId)}/cover?image_url=${encodeURIComponent(imageUrl)}`, thumbnailRequestTimeoutMs),
  getCatalogOwnership: (libraryId: string, sourceId: unknown, publications: unknown[]) => rpc("get_catalog_ownership", { libraryId, sourceId, publications }),
  importCatalogBook: async (destination: unknown, request: unknown, username: string | null, password: string | null) => {
    await rpc("import_catalog_book", { destination, request, username, password });
    return null;
  },
  getLibraryHome: async (libraryId: string, limit: number): Promise<LibraryHomeView> => {
    const [recentlyRead, recentlyAdded] = await Promise.all([
      rpc<LegacyBookSummary[]>("get_recently_read", { libraryId, limit }),
      rpc<LegacyBookSummary[]>("get_recently_added", { libraryId, limit }),
    ]);
    return { recently_read: recentlyRead.map(bookCard), recently_added: recentlyAdded.map(bookCard) };
  },
  searchLibrary: async (libraryId: string, query: string, limit: number) => (await rpc<LegacyBookSummary[]>("search_books", { libraryId, query, limit })).map(bookCard),
  getLibraryAuthors: async (libraryId: string, selectedAuthorId: string | null): Promise<LibraryAuthorsView> => {
    const authors = await rpc<AuthorSummary[]>("get_authors", { libraryId });
    if (selectedAuthorId === null) return { authors, books: null };
    const groups = await rpc<LegacyAuthorBooks[]>("get_author_folder_books", { libraryId, authorId: selectedAuthorId });
    return { authors, books: groups.flatMap((group) => group.books.map(bookCard)) };
  },
  discoverCatalogAuthorBooks: (libraryId: string, authorId: string) => rpc("discover_catalog_author_books", { libraryId, authorId }),
  getBookThumbnail: (libraryId: string, bookId: string) => binary(`/api/web/books/${encodeURIComponent(bookId)}/thumbnail?library_id=${encodeURIComponent(libraryId)}`, thumbnailRequestTimeoutMs),
  openBook: (libraryId: string, bookId: string) => binary(`/api/web/books/${encodeURIComponent(bookId)}/content?library_id=${encodeURIComponent(libraryId)}`),
  getBookFileName: (libraryId: string, bookId: string) => rpc("get_book_file_name", { libraryId, bookId }),
  getBookReadingPosition: (libraryId: string, bookId: string) => rpc("get_book_reading_position", { libraryId, bookId }),
  updateReadingPosition: async (libraryId: string, bookId: string, cfi: string, progress: number | null) => {
    const valid = await validateReadingPositionLocally(cfi);
    if (valid === false) throw new Error("The reader produced an invalid reading position.");
    return rpc("update_reading_position", { libraryId, bookId, cfi, progress });
  },
  getAnnotations: (libraryId: string, bookId: string) => rpc("get_annotations", { libraryId, bookId }),
  upsertAnnotation: (libraryId: string, annotation: unknown) => rpc("upsert_annotation", { libraryId, annotation }),
  deleteAnnotation: (libraryId: string, annotationId: string, modifiedAt: number | null) => rpc("delete_annotation", { libraryId, annotationId, modifiedAt }),
  getBookmarks: (libraryId: string, bookId: string) => rpc("get_bookmarks", { libraryId, bookId }),
  createBookmark: (libraryId: string, bookmark: unknown) => rpc("create_bookmark", { libraryId, bookmark }),
  deleteBookmark: (libraryId: string, bookmarkId: string, deletedAt: number | null) => rpc("delete_bookmark", { libraryId, bookmarkId, deletedAt }),
  getBookLinks: (libraryId: string, bookId: string) => rpc("get_book_links", { libraryId, bookId }),
  upsertBookLink: (libraryId: string, link: unknown) => rpc("upsert_book_link", { libraryId, link }),
  deleteBookLink: (libraryId: string, linkId: string, modifiedAt: number | null) => rpc("delete_book_link", { libraryId, linkId, modifiedAt }),
  downloadBook: (libraryId: string, bookId: string) => rpc("download_book", { libraryId, bookId }),
  removeLocalBookCopy: (libraryId: string, bookId: string) => rpc("remove_local_book_copy", { libraryId, bookId }),
  downloadFolder: (libraryId: string, dirId: string) => rpc("download_folder", { libraryId, dirId }),
};

export const webHttpCommands = implementation as unknown as typeof nativeCommands;
