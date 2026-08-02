import type {
  AccountStatus,
  AuthorFolderBookSummaries_Serialize,
  AuthorSummary,
  BookSummary_Serialize,
  BrowsingPreferencePatch,
  BrowsingPreferences_Serialize,
  Dir,
  DirSummaryContent_Serialize,
  LibraryEntry,
  NetworkStatus,
  ReaderPreferencePatch,
  ReaderPreferences,
  TransferPreferencePatch,
  TransferPreferences,
} from "./bindings";
import { FRONTEND_CONTRACT } from "./bindings";

const DEMO_LIBRARY_ID = "web-demo";
const rootFolders: Dir[] = [
  { id: "classics", name: "Classics", book_count: 3, downloaded_book_count: 3 },
  { id: "essays", name: "Essays", book_count: 2, downloaded_book_count: 2 },
  { id: "science", name: "Science & nature", book_count: 2, downloaded_book_count: 2 },
];

let customFolders: Dir[] = [];
let browsingPreferences: BrowsingPreferences_Serialize = { ...FRONTEND_CONTRACT.browsing_preferences.defaults };
let readerPreferences: ReaderPreferences = { ...FRONTEND_CONTRACT.reader_preferences.defaults };
let transferPreferences: TransferPreferences = { allow_background_on_metered: false };
let lastNetworkStatus: NetworkStatus = { transport: "unknown", connected: true, metered: null, validated: null };

const books: Array<BookSummary_Serialize & { folder: string }> = [
  { id: "demo-pride", title: "Pride and Prejudice", author: "Jane Austen", read_progress: 68, last_read: 8, added_at: 3, download_state: "Downloaded", folder: "classics" },
  { id: "demo-frankenstein", title: "Frankenstein", author: "Mary Shelley", read_progress: 24, last_read: 7, added_at: 6, download_state: "Downloaded", folder: "classics" },
  { id: "demo-odyssey", title: "The Odyssey", author: "Homer", read_progress: 0, last_read: 0, added_at: 7, download_state: "Downloaded", folder: "classics" },
  { id: "demo-room", title: "A Room of One’s Own", author: "Virginia Woolf", read_progress: 12, last_read: 6, added_at: 5, download_state: "Downloaded", folder: "essays" },
  { id: "demo-civil", title: "Civil Disobedience", author: "Henry David Thoreau", read_progress: 0, last_read: 0, added_at: 2, download_state: "Downloaded", folder: "essays" },
  { id: "demo-origin", title: "On the Origin of Species", author: "Charles Darwin", read_progress: 41, last_read: 5, added_at: 4, download_state: "Downloaded", folder: "science" },
  { id: "demo-relativity", title: "Relativity", author: "Albert Einstein", read_progress: 0, last_read: 0, added_at: 1, download_state: "Downloaded", folder: "science" },
];

const summary = ({ folder: _folder, ...book }: (typeof books)[number]): BookSummary_Serialize => book;
const summaries = (items = books) => items.map(summary);
const groupBooks = (items: typeof books, key: (book: (typeof books)[number]) => string) => items.reduce<Record<string, typeof books>>((groups, book) => {
  const value = key(book);
  (groups[value] ??= []).push(book);
  return groups;
}, {});
const unsupported = (feature: string): never => {
  throw new Error(`${feature} is available in the installed app. In this demo, choose “Open an EPUB” to read a local book.`);
};

export const webDemoCommands = {
  initLogging: async () => undefined,
  backendLinkStatus: async () => "web-demo",
  getLibraries: async (): Promise<LibraryEntry[]> => [{ library_id: DEMO_LIBRARY_ID, library_name: "Demo Library", library_path: "browser://demo", sync_binding: null }],
  getAccountStatus: async (): Promise<AccountStatus> => ({ signed_in: false, server_url: null, username: null }),
  login: async (_username: string, _password: string): Promise<AccountStatus> => unsupported("Account sign-in"),
  bootstrapRemoteLibraries: async () => unsupported("Cloud sync"),
  syncAllLibraries: async () => unsupported("Cloud sync"),
  getLibrarySyncStatuses: async () => [],
  reconcileLibraryCatalogue: async () => ({ libraries: await webDemoCommands.getLibraries(), sync_statuses: [] }),
  uploadAndSyncLibrary: async (_libraryId: string) => unsupported("Cloud sync"),
  disableLibrarySync: async (_libraryId: string) => unsupported("Cloud sync"),
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
  removeLibrary: async (_libraryId: string) => unsupported("Removing libraries"),
  selectLibrary: async (_libraryId: string) => null,
  getDirContents: async (dirId: string): Promise<DirSummaryContent_Serialize> => dirId === "__root__"
    ? { books: [], dirs: [...rootFolders, ...customFolders] }
    : { books: summaries(books.filter((book) => book.folder === dirId)), dirs: [] },
  createDirectory: async (_parentId: string, name: string): Promise<Dir> => {
    const folder = { id: `demo-folder-${Date.now()}`, name, book_count: 0, downloaded_book_count: 0 };
    customFolders = [...customFolders, folder];
    return folder;
  },
  importBook: async (_parentId: string, _sourcePath: string, _fileName: string): Promise<BookSummary_Serialize> => unsupported("Adding books to the library"),
  getOpdsEndpoints: async () => [],
  getOpdsCatalog: async () => unsupported("Online catalogs"),
  searchOpdsCatalog: async () => unsupported("Online catalogs"),
  importOpdsEpub: async () => unsupported("Catalog imports"),
  getRecentlyRead: async (limit: number) => summaries([...books].filter((book) => book.read_progress > 0).sort((a, b) => b.last_read - a.last_read).slice(0, limit)),
  getRecentlyAdded: async (limit: number) => summaries([...books].sort((a, b) => b.added_at - a.added_at).slice(0, limit)),
  searchBooks: async (query: string, limit: number) => {
    const needle = query.toLocaleLowerCase();
    return summaries(books.filter((book) => `${book.title} ${book.author}`.toLocaleLowerCase().includes(needle)).slice(0, limit));
  },
  getAuthors: async (): Promise<AuthorSummary[]> => Object.entries(groupBooks(books, (book) => book.author))
    .map(([name, authorBooks]) => ({ name, book_count: authorBooks.length }))
    .sort((a, b) => a.name.localeCompare(b.name)),
  getAuthorFolderBooks: async (authorName: string): Promise<AuthorFolderBookSummaries_Serialize[]> => Object.entries(groupBooks(books.filter((book) => book.author === authorName), (book) => book.folder))
    .map(([folderId, folderBooks]) => ({ folder_id: folderId, folder_path: rootFolders.find((folder) => folder.id === folderId)?.name ?? folderId, books: summaries(folderBooks) })),
  getBookThumbnail: async (_bookId: string) => [],
  openBook: async (_bookId: string) => unsupported("Opening sample titles"),
  getBookReadingPosition: async (_bookId: string) => null,
  updateReadingPosition: async (_bookId: string, _cfi: string) => null,
  updateLastRead: async (_bookId: string) => null,
  downloadBook: async (_bookId: string) => "Downloaded" as const,
  downloadFolder: async (dirId: string) => ({ dir_id: dirId, downloaded_books: books.filter((book) => book.folder === dirId).length, total_books: books.filter((book) => book.folder === dirId).length }),
  syncOnce: async () => null,
};
