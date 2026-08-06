import type {
  AccountStatus,
  BookCardRow,
  BookImportDestination,
  BookLink,
  BrowsingPreferencePatch,
  BrowsingPreferences_Serialize,
  Dir,
  LibraryAuthorsView,
  LibraryFolderView,
  LibraryHomeView,
  LibraryEntry,
  NetworkStatus,
  ReaderPreferencePatch,
  ReaderPreferences,
  TransferPreferencePatch,
  TransferPreferences,
} from "./bindings";
import { FRONTEND_CONTRACT } from "./bindings";
import { DEMO_OVERDRIVE_SOURCE, demoOverdrivePage, demoWideCatalogSearch } from "./catalog-demo";

const DEMO_LIBRARY_ID = "web-demo";
const rootFolders: Dir[] = [
  { id: "classics", name: "Classics", book_count: 3, downloaded_book_count: 3 },
  { id: "essays", name: "Essays", book_count: 2, downloaded_book_count: 2 },
  { id: "science", name: "Science & nature", book_count: 2, downloaded_book_count: 2 },
];

let customFolders: Dir[] = [];
let browsingPreferences: BrowsingPreferences_Serialize = { ...FRONTEND_CONTRACT.browsing_preferences.defaults };
let readerPreferences: ReaderPreferences = { ...FRONTEND_CONTRACT.reader_preferences.defaults };
let transferPreferences: TransferPreferences = { allow_background_on_metered: false, auto_download_limit_mb: null };
let lastNetworkStatus: NetworkStatus = { transport: "unknown", connected: true, metered: null, validated: null };
let bookLinks: BookLink[] = [];

export function resetWebDemoState() {
  customFolders = [];
  browsingPreferences = { ...FRONTEND_CONTRACT.browsing_preferences.defaults };
  readerPreferences = { ...FRONTEND_CONTRACT.reader_preferences.defaults };
  transferPreferences = { allow_background_on_metered: false, auto_download_limit_mb: null };
  lastNetworkStatus = { transport: "unknown", connected: true, metered: null, validated: null };
  bookLinks = [];
}

const books = [
  { id: "demo-pride", title: "Pride and Prejudice", author: "Jane Austen", read_progress: 68, last_read: 8, added_at: 3, download_state: "Downloaded", folder: "classics" },
  { id: "demo-frankenstein", title: "Frankenstein", author: "Mary Shelley", read_progress: 24, last_read: 7, added_at: 6, download_state: "Downloaded", folder: "classics" },
  { id: "demo-odyssey", title: "The Odyssey", author: "Homer", read_progress: 0, last_read: 0, added_at: 7, download_state: "Downloaded", folder: "classics" },
  { id: "demo-room", title: "A Room of One’s Own", author: "Virginia Woolf", read_progress: 12, last_read: 6, added_at: 5, download_state: "Downloaded", folder: "essays" },
  { id: "demo-civil", title: "Civil Disobedience", author: "Henry David Thoreau", read_progress: 0, last_read: 0, added_at: 2, download_state: "Downloaded", folder: "essays" },
  { id: "demo-origin", title: "On the Origin of Species", author: "Charles Darwin", read_progress: 41, last_read: 5, added_at: 4, download_state: "Downloaded", folder: "science" },
  { id: "demo-relativity", title: "Relativity", author: "Albert Einstein", read_progress: 0, last_read: 0, added_at: 1, download_state: "Downloaded", folder: "science" },
] satisfies Array<{ id: string; title: string; author: string; read_progress: number; last_read: number; added_at: number; download_state: "Downloaded"; folder: string }>;

const card = (book: (typeof books)[number]): BookCardRow => ({
  book_id: book.id,
  title: book.title,
  author: book.author,
  description: "",
  progress: book.read_progress,
  downloaded: book.download_state === "Downloaded",
  download_requested: false,
  audiobook: false,
  audiobook_duration_ms: null,
  audiobook_chapter_count: null,
});
const cards = (items = books) => items.map(card);
const groupBooks = (items: typeof books, key: (book: (typeof books)[number]) => string) => items.reduce<Record<string, typeof books>>((groups, book) => {
  const value = key(book);
  (groups[value] ??= []).push(book);
  return groups;
}, {});
const unsupported = (feature: string): never => {
  throw new Error(`${feature} is not available in the standalone demo. Choose “Open an EPUB” to read a local book.`);
};

export const webDemoCommands = {
  initLogging: async () => undefined,
  backendLinkStatus: async () => "standalone-demo",
  getLibraries: async (): Promise<LibraryEntry[]> => [{ library_id: DEMO_LIBRARY_ID, library_name: "Demo Library", library_path: "browser://demo" }],
  getAccountStatus: async (): Promise<AccountStatus> => ({ signed_in: false, server_url: null, username: null }),
  getAccountSessions: async () => [],
  revokeAccountSession: async (_sessionId: string): Promise<AccountStatus> => ({ signed_in: false, server_url: null, username: null }),
  revokeOtherAccountSessions: async () => 0,
  login: async (_username: string, _password: string): Promise<AccountStatus> => unsupported("Account sign-in"),
  requestPublicRegistration: async (_email: string, _username: string, _password: string): Promise<null> => unsupported("Account registration"),
  resendVerification: async (_email: string): Promise<null> => unsupported("Account verification"),
  verifyEmail: async (_token: string): Promise<AccountStatus> => unsupported("Account verification"),
  requestPasswordReset: async (_email: string): Promise<null> => unsupported("Password recovery"),
  resetPassword: async (_token: string, _newPassword: string): Promise<null> => unsupported("Password recovery"),
  bootstrapRemoteLibraries: async () => unsupported("Cloud sync"),
  syncAllLibraries: async () => unsupported("Cloud sync"),
  getLibrarySyncStatuses: async () => [],
  reconcileLibraryCatalogue: async () => ({ libraries: await webDemoCommands.getLibraries(), sync_statuses: [] }),
  getTransferStatuses: async () => [],
  getNetworkStatus: async () => lastNetworkStatus,
  updateNetworkStatus: async (status: NetworkStatus) => (lastNetworkStatus = status),
  getTransferPreferences: async () => transferPreferences,
  updateTransferPreference: async (patch: TransferPreferencePatch) => (transferPreferences = { ...transferPreferences, [patch.preference]: patch.value }),
  logout: async (): Promise<AccountStatus> => ({ signed_in: false, server_url: null, username: null }),
  getReaderPreferences: async () => readerPreferences,
  updateReaderPreference: async (patch: ReaderPreferencePatch) => (readerPreferences = { ...readerPreferences, [patch.preference]: patch.value } as ReaderPreferences),
  getBrowsingPreferences: async () => browsingPreferences,
  updateBrowsingPreference: async (patch: BrowsingPreferencePatch) => (browsingPreferences = { ...browsingPreferences, [patch.preference]: patch.value } as BrowsingPreferences_Serialize),
  createLibrary: async (_name: string): Promise<LibraryEntry> => unsupported("Creating libraries"),
  openLibraryFolder: async (_path: string): Promise<LibraryEntry> => unsupported("Opening library folders"),
  removeLibrary: async (_libraryId: string) => unsupported("Removing libraries"),
  getLibraryFolder: async (_libraryId: string, dirId: string): Promise<LibraryFolderView> => dirId === "__root__"
    ? { books: [], directories: [...rootFolders, ...customFolders] }
    : { books: cards(books.filter((book) => book.folder === dirId)), directories: [] },
  getChildDirectories: async (_libraryId: string, parentId: string): Promise<Dir[]> => parentId === "__root__" ? [...rootFolders, ...customFolders] : [],
  createDirectory: async (_libraryId: string, _parentId: string, name: string): Promise<Dir> => {
    const folder = { id: `demo-folder-${Date.now()}`, name, book_count: 0, downloaded_book_count: 0 };
    customFolders = [...customFolders, folder];
    return folder;
  },
  importBook: async (_destination: BookImportDestination, _sourcePath: string, _fileName: string): Promise<null> => unsupported("Adding books to the library"),
  getOpdsEndpoints: async () => [],
  refreshOpdsEndpoints: async () => [],
  getOpdsCatalog: async () => unsupported("Online catalogs"),
  searchOpdsCatalog: async () => unsupported("Online catalogs"),
  getOpdsOwnership: async () => [],
  importOpdsBook: async () => unsupported("Catalog imports"),
  getCatalogSources: async () => [DEMO_OVERDRIVE_SOURCE],
  refreshCatalogSources: async () => [DEMO_OVERDRIVE_SOURCE],
  getCustomCatalogSources: async () => [],
  addCustomCatalog: async () => unsupported("Custom catalogs"),
  removeCustomCatalog: async () => unsupported("Custom catalogs"),
  searchCatalogs: async (query: string) => demoWideCatalogSearch(query),
  getCatalogPage: async () => demoOverdrivePage(),
  searchCatalog: async (_sourceId: unknown, _search: unknown, query: Parameters<typeof demoOverdrivePage>[0]) => demoOverdrivePage(query),
  getCatalogCover: async () => [],
  getCatalogOwnership: async () => [],
  importCatalogBook: async () => unsupported("Catalog imports"),
  getLibraryHome: async (_libraryId: string, limit: number): Promise<LibraryHomeView> => ({
    recently_read: cards([...books].filter((book) => book.read_progress > 0).sort((a, b) => b.last_read - a.last_read).slice(0, limit)),
    recently_added: cards([...books].sort((a, b) => b.added_at - a.added_at).slice(0, limit)),
  }),
  searchLibrary: async (_libraryId: string, query: string, limit: number) => {
    const needle = query.toLocaleLowerCase();
    return cards(books.filter((book) => book.title.toLocaleLowerCase().includes(needle)).slice(0, limit));
  },
  getLibraryAuthors: async (_libraryId: string, selectedAuthorId: string | null): Promise<LibraryAuthorsView> => ({
    authors: Object.entries(groupBooks(books, (book) => book.author))
      .map(([name, authorBooks]) => ({ id: name, name, book_count: authorBooks.length }))
      .sort((a, b) => a.name.localeCompare(b.name)),
    books: selectedAuthorId === null ? null : cards(books.filter((book) => book.author === selectedAuthorId)),
  }),
  discoverCatalogAuthorBooks: async (_libraryId: string, authorId: string) => ({ author_name: authorId, matches: [] }),
  getBookThumbnail: async (_libraryId: string, _bookId: string) => [],
  openBook: async (_libraryId: string, _bookId: string) => unsupported("Opening sample titles"),
  getBookFileName: async (_libraryId: string, bookId: string) => `${bookId}.epub`,
  getBookReadingPosition: async (_libraryId: string, _bookId: string) => null,
  updateReadingPosition: async (_libraryId: string, _bookId: string, _cfi: string, _progress: number) => null,
  updateLastRead: async (_libraryId: string, _bookId: string) => null,
  getAnnotations: async (_libraryId: string, _bookId: string) => [],
  upsertAnnotation: async (_libraryId: string, _annotation: unknown) => null,
  deleteAnnotation: async (_libraryId: string, _annotationId: string, _modifiedAt: number) => null,
  getBookmarks: async (_libraryId: string, _bookId: string) => [],
  upsertBookmark: async (_libraryId: string, _bookmark: unknown) => null,
  deleteBookmark: async (_libraryId: string, _bookmarkId: string, _modifiedAt: number) => null,
  getBookLinks: async (_libraryId: string, bookId: string) => bookLinks
    .filter((link) => link.source_book_id === bookId || link.target_book_id === bookId)
    .sort((left, right) => right.modified_at - left.modified_at),
  upsertBookLink: async (_libraryId: string, link: BookLink) => {
    bookLinks = [link, ...bookLinks.filter((item) => item.id !== link.id)];
    return null;
  },
  deleteBookLink: async (_libraryId: string, linkId: string, _modifiedAt: number | null) => {
    bookLinks = bookLinks.filter((link) => link.id !== linkId);
    return null;
  },
  downloadBook: async (_libraryId: string, _bookId: string) => "Downloaded" as const,
  removeLocalBookCopy: async (_libraryId: string, _bookId: string) => "NotDownloaded" as const,
  downloadFolder: async (_libraryId: string, dirId: string) => ({ dir_id: dirId, downloaded_books: books.filter((book) => book.folder === dirId).length, total_books: books.filter((book) => book.folder === dirId).length }),
};
