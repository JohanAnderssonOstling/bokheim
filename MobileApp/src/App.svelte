<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";

  import Reader from "./components/Reader.svelte";
  import BookGrid from "./components/library/BookGrid.svelte";
  import BookShelf from "./components/library/BookShelf.svelte";
  import CreateFolderDialog from "./components/library/CreateFolderDialog.svelte";
  import FolderCard from "./components/library/FolderCard.svelte";
  import AdaptiveNavigation from "./components/shell/AdaptiveNavigation.svelte";
  import AppTopbar from "./components/shell/AppTopbar.svelte";
  import BrowsingSettingsDialog from "./components/shell/BrowsingSettingsDialog.svelte";
  import LibrarySwitcher from "./components/shell/LibrarySwitcher.svelte";
  import Store from "./components/store/Store.svelte";
  import { applyApplicationAppearance } from "./lib/application-appearance";
  import { commands } from "./lib/bindings";
  import { pickEpub, type PickedEpub } from "./lib/epub-picker";
  import { NAV_ITEMS, type Destination } from "./lib/navigation";
  import { observeNetworkStatus, UNKNOWN_NETWORK_STATUS } from "./lib/network-status";
  import { commitAndMeasureRender, measureInvoke } from "./lib/performance";
  import { clearThumbnailCache } from "./lib/thumbnail-cache";
  import {
    DEFAULT_READER_PREFERENCES,
    DEFAULT_BROWSING_PREFERENCES,
    type AccountStatus,
    type AuthorFolderBooks,
    type AuthorSummary,
    type BookSummary,
    type BrowsingPreferencePatch,
    type BrowsingPreferences,
    type DirectoryContent,
    type DownloadState,
    type FolderDownloadStatus,
    type LibraryDirectory,
    type LibraryEntry,
    type LibrarySyncStatus,
    type LibraryTransfers,
    type NetworkStatus,
    type ReaderBook,
    type ReaderPreferencePatch,
    type ReaderPreferences,
    type TransferPreferencePatch,
    type TransferPreferences,
  } from "./lib/types";
  import { APP_NAME, APP_NAME_LOWER } from "./lib/application-name";

  type FolderLevel = { id: string; name: string };
  type BookBytes = ArrayBuffer | Uint8Array | number[];
  type DownloadStatusEvent = {
    library_id: string;
    event: { DownloadStatusChanged: { book_id: string; state: DownloadState } };
  };
  type FolderDownloadStatusEvent = {
    library_id: string;
    event: { FolderDownloadStatusChanged: { dir_id: string; downloaded_books: number; total_books: number } };
  };

  const EMPTY_DIRECTORY: DirectoryContent = { books: [], dirs: [] };
  const HOME_BOOK_LIMIT = 20;
  const AUTHOR_PAGE_SIZE = 80;
  const WEB_PREFERENCES_KEY = `${APP_NAME_LOWER}-reader-preferences`;

  let libraries = $state<LibraryEntry[]>([]);
  let activeLibraryId = $state("");
  let activeDestination = $state<Destination>("home");
  let recentlyAdded = $state<BookSummary[]>([]);
  let recentlyRead = $state<BookSummary[]>([]);
  let directoryContent = $state<DirectoryContent>(EMPTY_DIRECTORY);
  let folderStack = $state<FolderLevel[]>([]);
  let authors = $state<AuthorSummary[]>([]);
  let authorFilter = $state("");
  let focusedAuthorIndex = $state(0);
  let selectedAuthor = $state("");
  let authorGroups = $state<AuthorFolderBooks[]>([]);
  let searchQuery = $state("");
  let searchResults = $state<BookSummary[]>([]);
  let searchPending = $state(false);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let showLibrarySwitcher = $state(false);
  let showCreateFolder = $state(false);
  let showBrowsingSettings = $state(false);
  let readerBook = $state<ReaderBook | null>(null);
  let loadingLibrary = $state(true);
  let homeLoading = $state(false);
  let folderLoading = $state(false);
  let authorsLoading = $state(false);
  let authorBooksLoading = $state(false);
  let homeLoadedFor = $state("");
  let folderLoadedFor = $state("");
  let authorsLoadedFor = $state("");
  let visibleAuthorCount = $state(AUTHOR_PAGE_SIZE);
  let libraryGeneration = 0;
  let folderRequest = 0;
  let authorRequest = 0;
  let openingBookId = $state("");
  let bookDownloadStates = $state<Record<string, DownloadState>>({});
  let folderDownloadStatuses = $state<Record<string, FolderDownloadStatus>>({});
  let activeFolderDownloads = $state<Record<string, boolean>>({});
  let libraryError = $state("");
  let newLibraryName = $state("");
  let creatingLibrary = $state(false);
  let addingBook = $state(false);
  let browsingPreferences = $state<BrowsingPreferences>({ ...DEFAULT_BROWSING_PREFERENCES });
  let readerPreferences = $state<ReaderPreferences>({ ...DEFAULT_READER_PREFERENCES });
  let accountStatus = $state<AccountStatus>({ signed_in: false, server_url: null, username: null });
  let accountBusy = $state(false);
  let accountProgress = $state("");
  let accountError = $state("");
  let librarySyncStatuses = $state<LibrarySyncStatus[]>([]);
  let libraryTransfers = $state<LibraryTransfers[]>([]);
  let librarySyncBusyId = $state("");
  let networkStatus = $state<NetworkStatus>({ ...UNKNOWN_NETWORK_STATUS });
  let transferPreferences = $state<TransferPreferences>({ allow_background_on_metered: false });
  let browsingPreferenceSaveQueue = Promise.resolve();
  let browsingPreferenceVersion = 0;
  let preferenceSaveQueue = Promise.resolve();
  let preferenceVersion = 0;

  const destinationTitle = $derived(NAV_ITEMS.find((item) => item.id === activeDestination)?.label ?? APP_NAME);
  const activeLibrary = $derived(libraries.find((library) => library.library_id === activeLibraryId));
  const authorBooks = $derived(authorGroups.flatMap((group) => group.books));
  const filteredAuthors = $derived(authors.filter((author) => author.name.toLocaleLowerCase().includes(authorFilter.trim().toLocaleLowerCase())));
  const visibleAuthors = $derived(filteredAuthors.slice(0, visibleAuthorCount));

  $effect(() => {
    applyApplicationAppearance(browsingPreferences);
  });

  $effect(() => {
    authorFilter;
    visibleAuthorCount = AUTHOR_PAGE_SIZE;
    focusedAuthorIndex = 0;
  });

  $effect(() => {
    if (!showBrowsingSettings || !accountStatus.signed_in) return;
    void loadServerTransferState().catch((reason) => (accountError = errorMessage(reason)));
    const timer = window.setInterval(() => {
      void loadTransferStatuses().catch(() => undefined);
    }, 1200);
    return () => window.clearInterval(timer);
  });

  function errorMessage(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  function setDestination(destination: Destination) {
    activeDestination = destination;
    libraryError = "";
    void ensureDestination(destination);
    if (destination === "search") {
      requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#library-search")?.focus());
    }
  }

  function resetLibraryData() {
    folderRequest += 1;
    authorRequest += 1;
    homeLoading = false;
    folderLoading = false;
    authorsLoading = false;
    authorBooksLoading = false;
    recentlyAdded = [];
    recentlyRead = [];
    directoryContent = EMPTY_DIRECTORY;
    authors = [];
    authorFilter = "";
    focusedAuthorIndex = 0;
    selectedAuthor = "";
    authorGroups = [];
    folderStack = [];
    searchQuery = "";
    searchResults = [];
    visibleAuthorCount = AUTHOR_PAGE_SIZE;
    homeLoadedFor = "";
    folderLoadedFor = "";
    authorsLoadedFor = "";
    bookDownloadStates = {};
    folderDownloadStatuses = {};
    activeFolderDownloads = {};
  }

  function mergeBookDownloadStates(books: BookSummary[]) {
    const next = { ...bookDownloadStates };
    for (const book of books) next[book.id] = book.download_state;
    bookDownloadStates = next;
  }

  function replaceFolderDownloadStates(folders: LibraryDirectory[]) {
    folderDownloadStatuses = Object.fromEntries(
      folders
        .filter((folder) => folder.id !== "__root__")
        .map((folder) => [folder.id, { dir_id: folder.id, downloaded_books: folder.downloaded_book_count, total_books: folder.book_count }]),
    );
  }

  async function downloadBook(book: BookSummary) {
    if (bookDownloadStates[book.id] === "Downloaded") return Promise.resolve();
    bookDownloadStates = { ...bookDownloadStates, [book.id]: "Queued" };
    try {
      const state = await commands.downloadBook(book.id);
      bookDownloadStates = { ...bookDownloadStates, [book.id]: state };
    } catch (reason) {
      libraryError = errorMessage(reason);
      throw reason;
    }
  }

  async function downloadFolder(folder: LibraryDirectory) {
    if (activeFolderDownloads[folder.id]) return;
    activeFolderDownloads = { ...activeFolderDownloads, [folder.id]: true };
    libraryError = "";
    try {
      const status = await commands.downloadFolder(folder.id);
      folderDownloadStatuses = { ...folderDownloadStatuses, [folder.id]: status };
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      activeFolderDownloads = { ...activeFolderDownloads, [folder.id]: false };
    }
  }

  function clearSelectedAuthor() {
    authorRequest += 1;
    selectedAuthor = "";
    authorGroups = [];
    authorBooksLoading = false;
    requestAnimationFrame(() => document.querySelector<HTMLButtonElement>(`[data-author-index="${focusedAuthorIndex}"]`)?.focus());
  }

  function focusAuthor(index: number) {
    const lastVisibleIndex = Math.min(filteredAuthors.length, visibleAuthorCount) - 1;
    if (lastVisibleIndex < 0) return;
    focusedAuthorIndex = Math.max(0, Math.min(index, lastVisibleIndex));
    requestAnimationFrame(() => document.querySelector<HTMLButtonElement>(`[data-author-index="${focusedAuthorIndex}"]`)?.focus());
  }

  function handleAuthorFilterKeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown" && visibleAuthors.length) {
      event.preventDefault();
      focusAuthor(0);
    } else if (event.key === "Escape" && authorFilter) {
      event.preventDefault();
      authorFilter = "";
    }
  }

  function handleAuthorKeydown(event: KeyboardEvent, index: number) {
    let nextIndex: number | undefined;
    if (event.key === "ArrowDown") {
      nextIndex = index + 1;
      if (nextIndex >= visibleAuthors.length && visibleAuthorCount < filteredAuthors.length) {
        visibleAuthorCount = Math.min(filteredAuthors.length, visibleAuthorCount + AUTHOR_PAGE_SIZE);
      }
    } else if (event.key === "ArrowUp") {
      nextIndex = index - 1;
    } else if (event.key === "Home") {
      nextIndex = 0;
    } else if (event.key === "End") {
      nextIndex = visibleAuthors.length - 1;
    } else {
      return;
    }
    event.preventDefault();
    focusAuthor(nextIndex);
  }

  async function ensureDestination(destination: Destination, generation = libraryGeneration) {
    if (!activeLibraryId || loadingLibrary || generation !== libraryGeneration) return;
    if (destination === "home" && homeLoadedFor !== activeLibraryId) await loadHome(generation);
    if (destination === "library") {
      const dirId = folderStack.at(-1)?.id ?? "__root__";
      if (folderLoadedFor !== `${activeLibraryId}:${dirId}`) await loadDirectory(dirId, folderStack, generation);
    }
    if (destination === "authors" && authorsLoadedFor !== activeLibraryId) await loadAuthors(generation);
  }

  async function loadHome(generation: number, force = false) {
    const libraryId = activeLibraryId;
    if (!libraryId || homeLoading || (!force && homeLoadedFor === libraryId)) return;
    homeLoading = true;
    try {
      const [added, read] = await Promise.all([
        measureInvoke("recently_added", () => commands.getRecentlyAdded(HOME_BOOK_LIMIT)),
        measureInvoke("recently_read", () => commands.getRecentlyRead(HOME_BOOK_LIMIT)),
      ]);
      if (generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      await commitAndMeasureRender("home", () => {
        recentlyAdded = added;
        recentlyRead = read;
        homeLoadedFor = libraryId;
      }, added.length + read.length);
      mergeBookDownloadStates([...added, ...read]);
    } catch (reason) {
      if (generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (generation === libraryGeneration) homeLoading = false;
    }
  }

  async function loadDirectory(dirId: string, nextStack: FolderLevel[], generation = libraryGeneration, force = false) {
    const libraryId = activeLibraryId;
    const request = ++folderRequest;
    if (!libraryId || (!force && folderLoadedFor === `${libraryId}:${dirId}`)) return;
    folderLoading = true;
    libraryError = "";
    try {
      const content = await measureInvoke("directory", () => commands.getDirContents(dirId));
      if (request !== folderRequest || generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      await commitAndMeasureRender("directory", () => {
        directoryContent = content;
        folderStack = nextStack;
        folderLoadedFor = `${libraryId}:${dirId}`;
      }, content.books.length + content.dirs.length);
      mergeBookDownloadStates(content.books);
      replaceFolderDownloadStates(content.dirs);
    } catch (reason) {
      if (request === folderRequest && generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (request === folderRequest && generation === libraryGeneration) folderLoading = false;
    }
  }

  async function loadAuthors(generation: number, force = false) {
    const libraryId = activeLibraryId;
    if (!libraryId || authorsLoading || (!force && authorsLoadedFor === libraryId)) return;
    authorsLoading = true;
    try {
      const authorList = await measureInvoke("authors", () => commands.getAuthors());
      if (generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      await commitAndMeasureRender("authors", () => {
        authors = authorList;
        visibleAuthorCount = AUTHOR_PAGE_SIZE;
        authorsLoadedFor = libraryId;
      }, authorList.length);
    } catch (reason) {
      if (generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (generation === libraryGeneration) authorsLoading = false;
    }
  }

  async function selectLibrary(libraryId: string) {
    const generation = ++libraryGeneration;
    showLibrarySwitcher = false;
    showCreateFolder = false;
    activeLibraryId = libraryId;
    libraryError = "";
    loadingLibrary = true;
    resetLibraryData();
    try {
      await measureInvoke("select_library", () => commands.selectLibrary(libraryId));
      if (generation !== libraryGeneration) return;
      loadingLibrary = false;
      await ensureDestination(activeDestination, generation);
    } catch (reason) {
      if (generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (generation === libraryGeneration) loadingLibrary = false;
    }
  }

  async function loadLibraries() {
    try {
      const entries = await measureInvoke("libraries", () => commands.getLibraries());
      await commitAndMeasureRender("libraries", () => (libraries = entries), entries.length);
      if (entries.length) await selectLibrary(entries[0].library_id);
      else loadingLibrary = false;
    } catch (reason) {
      // The standalone browser build still supports opening a local EPUB.
      libraryError = errorMessage(reason);
      loadingLibrary = false;
    }
  }

  async function loadAccountStatus() {
    try {
      accountStatus = await commands.getAccountStatus();
      if (accountStatus.signed_in) await loadServerTransferState();
    } catch {
      accountStatus = { signed_in: false, server_url: null, username: null };
    }
  }

  async function loadServerTransferState() {
    if (!accountStatus.signed_in) {
      librarySyncStatuses = [];
      libraryTransfers = [];
      return;
    }
    const [catalogue, transfers] = await Promise.all([
      commands.reconcileLibraryCatalogue(),
      commands.getTransferStatuses(),
    ]);
    libraries = catalogue.libraries;
    librarySyncStatuses = catalogue.sync_statuses;
    libraryTransfers = transfers;
  }

  async function loadTransferStatuses() {
    if (!accountStatus.signed_in) {
      libraryTransfers = [];
      return;
    }
    libraryTransfers = await commands.getTransferStatuses();
  }

  async function loadTransferPreferences() {
    try {
      transferPreferences = await commands.getTransferPreferences();
    } catch {
      transferPreferences = { allow_background_on_metered: false };
    }
  }

  async function saveTransferPreference(patch: TransferPreferencePatch) {
    const previous = transferPreferences;
    transferPreferences = { ...transferPreferences, [patch.preference]: patch.value };
    try {
      transferPreferences = await commands.updateTransferPreference(patch);
    } catch (reason) {
      transferPreferences = previous;
      accountError = errorMessage(reason);
    }
  }

  async function loginToServer(username: string, password: string) {
    if (accountBusy) return;
    accountBusy = true;
    accountError = "";
    accountProgress = "Signing in…";
    try {
      accountStatus = await commands.login(username, password);
      accountProgress = "Fetching libraries…";
      await loadServerTransferState();
      accountProgress = "";
      clearThumbnailCache();
      await loadLibraries();
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      accountBusy = false;
    }
  }

  async function syncServerLibraries() {
    if (accountBusy) return;
    accountBusy = true;
    accountError = "";
    accountProgress = "Syncing libraries…";
    try {
      await commands.bootstrapRemoteLibraries();
      clearThumbnailCache();
      await loadLibraries();
      await loadServerTransferState();
      accountProgress = "Up to date";
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      accountBusy = false;
    }
  }

  async function logoutFromServer() {
    if (accountBusy) return;
    accountBusy = true;
    accountError = "";
    accountProgress = "Signing out…";
    try {
      accountStatus = await commands.logout();
      accountProgress = "";
      librarySyncStatuses = [];
      libraryTransfers = [];
      await loadLibraries();
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      accountBusy = false;
    }
  }

  async function syncLibraryToServer(libraryId: string) {
    if (librarySyncBusyId || accountBusy) return;
    librarySyncBusyId = libraryId;
    accountError = "";
    accountProgress = "Preparing library…";
    try {
      await commands.uploadAndSyncLibrary(libraryId);
      accountProgress = "Up to date";
      await loadServerTransferState();
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      librarySyncBusyId = "";
    }
  }

  async function createFirstLibrary(event: SubmitEvent) {
    event.preventDefault();
    if (creatingLibrary) return;

    creatingLibrary = true;
    libraryError = "";
    try {
      const library = await measureInvoke("create_library", () => commands.createLibrary(newLibraryName.trim() || "Library"));
      libraries = [library];
      newLibraryName = "";
      await selectLibrary(library.library_id);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      creatingLibrary = false;
    }
  }

  async function openFolder(folder: LibraryDirectory) {
    await loadDirectory(folder.id, [...folderStack, { id: folder.id, name: folder.name }]);
  }

  async function goToFolder(level: number) {
    const nextStack = folderStack.slice(0, level);
    const dirId = nextStack.at(-1)?.id ?? "__root__";
    await loadDirectory(dirId, nextStack);
  }

  async function createFolder(name: string) {
    const parentId = folderStack.at(-1)?.id ?? "__root__";
    const created = await measureInvoke("create_directory", () => commands.createDirectory(parentId, name));
    const dirs = directoryContent.dirs.filter((folder) => folder.id !== created.id);
    directoryContent = {
      ...directoryContent,
      dirs: [...dirs, created].sort((left, right) => left.name.localeCompare(right.name)),
    };
  }

  async function selectAuthor(authorName: string) {
    const request = ++authorRequest;
    const generation = libraryGeneration;
    selectedAuthor = authorName;
    authorGroups = [];
    authorBooksLoading = true;
    libraryError = "";
    try {
      const groups = await measureInvoke("author_books", () => commands.getAuthorFolderBooks(authorName));
      if (request !== authorRequest || generation !== libraryGeneration || authorName !== selectedAuthor) return;
      await commitAndMeasureRender("author_books", () => (authorGroups = groups), groups.reduce((count, group) => count + group.books.length, 0));
      mergeBookDownloadStates(groups.flatMap((group) => group.books));
    } catch (reason) {
      if (request === authorRequest && generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (request === authorRequest && generation === libraryGeneration) authorBooksLoading = false;
    }
  }

  function scheduleSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    const query = searchQuery.trim();
    if (!query) {
      searchResults = [];
      searchPending = false;
      return;
    }
    searchPending = true;
    searchTimer = setTimeout(() => void runSearch(query), 220);
  }

  async function runSearch(query: string) {
    try {
      const results = await measureInvoke("search", () => commands.searchBooks(query, 100));
      if (query === searchQuery.trim()) {
        await commitAndMeasureRender("search", () => (searchResults = results), results.length);
        mergeBookDownloadStates(results);
      }
    } catch (reason) {
      if (query === searchQuery.trim()) libraryError = errorMessage(reason);
    } finally {
      if (query === searchQuery.trim()) searchPending = false;
    }
  }

  async function loadReaderPreferences() {
    try {
      readerPreferences = await measureInvoke("reader_preferences", () => commands.getReaderPreferences());
    } catch {
      const saved = localStorage.getItem(WEB_PREFERENCES_KEY);
      if (saved) {
        try {
          readerPreferences = { ...DEFAULT_READER_PREFERENCES, ...JSON.parse(saved) };
        } catch {
          readerPreferences = { ...DEFAULT_READER_PREFERENCES };
        }
      }
    }
  }

  async function loadBrowsingPreferences() {
    try {
      browsingPreferences = await measureInvoke("browsing_preferences", () => commands.getBrowsingPreferences());
    } catch {
      browsingPreferences = { ...DEFAULT_BROWSING_PREFERENCES };
    }
  }

  function saveBrowsingPreference(patch: BrowsingPreferencePatch) {
    const previous = browsingPreferences;
    const preferences = { ...browsingPreferences, [patch.preference]: patch.value } as BrowsingPreferences;
    browsingPreferences = preferences;
    const version = ++browsingPreferenceVersion;
    browsingPreferenceSaveQueue = browsingPreferenceSaveQueue.then(async () => {
      try {
        const saved = await commands.updateBrowsingPreference(patch);
        if (version === browsingPreferenceVersion) browsingPreferences = saved;
      } catch {
        if (version === browsingPreferenceVersion) browsingPreferences = previous;
      }
    });
    return browsingPreferenceSaveQueue;
  }

  function saveReaderPreference(patch: ReaderPreferencePatch) {
    const preferences = { ...readerPreferences, [patch.preference]: patch.value } as ReaderPreferences;
    readerPreferences = preferences;
    const version = ++preferenceVersion;
    preferenceSaveQueue = preferenceSaveQueue.then(async () => {
      try {
        const saved = await commands.updateReaderPreference(patch);
        if (version === preferenceVersion) readerPreferences = saved;
      } catch {
        localStorage.setItem(WEB_PREFERENCES_KEY, JSON.stringify(preferences));
      }
    });
    return preferenceSaveQueue;
  }

  async function openLibraryBook(book: BookSummary) {
    openingBookId = book.id;
    libraryError = "";
    try {
      if ((bookDownloadStates[book.id] ?? book.download_state) !== "Downloaded") {
        await measureInvoke("download_book", () => downloadBook(book));
        if (bookDownloadStates[book.id] !== "Downloaded") {
          throw new Error("This book is queued for download and is not available offline yet.");
        }
      }
      const [rawBytes, initialCfi] = await Promise.all([
        measureInvoke("open_book", () => commands.openBook(book.id) as unknown as Promise<BookBytes>),
        measureInvoke("reading_position", () => commands.getBookReadingPosition(book.id)),
      ]);
      const buffer = rawBytes instanceof ArrayBuffer
        ? rawBytes
        : rawBytes instanceof Uint8Array
          ? rawBytes.slice().buffer
          : Uint8Array.from(rawBytes).buffer;
      const title = book.title || "Untitled";
      const file = new File([buffer], `${title}.epub`, { type: "application/epub+zip" });
      await measureInvoke("update_last_read", () => commands.updateLastRead(book.id));
      readerBook = {
        key: `library:${activeLibraryId}:${book.id}`,
        file,
        title,
        author: book.author,
        initialCfi,
        saveCfi: async (cfi) => { await measureInvoke("update_reading_position", () => commands.updateReadingPosition(book.id, cfi)); },
      };
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      openingBookId = "";
    }
  }

  async function openDeviceFile() {
    libraryError = "";
    let picked: PickedEpub | null;
    try {
      picked = await pickEpub();
    } catch (reason) {
      libraryError = errorMessage(reason);
      return;
    }
    if (!picked) return;
    try {
      const file = await picked.asFile();
      const storageKey = `foliate-position:${file.name}:${file.size}:${file.lastModified}`;
      readerBook = {
        key: storageKey,
        file,
        title: file.name.replace(/\.epub$/i, ""),
        initialCfi: localStorage.getItem(storageKey),
        saveCfi: (cfi) => localStorage.setItem(storageKey, cfi),
      };
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      await picked.dispose();
    }
  }

  async function addBookToCurrentFolder() {
    if (addingBook) return;
    libraryError = "";
    let picked: PickedEpub | null;
    try {
      picked = await pickEpub();
    } catch (reason) {
      libraryError = errorMessage(reason);
      return;
    }
    if (!picked) return;
    addingBook = true;
    try {
      const sourcePath = await picked.asImportPath();
      if (!sourcePath) throw new Error("Adding books requires the native application.");
      const parentId = folderStack.at(-1)?.id ?? "__root__";
      await measureInvoke("import_book", () => commands.importBook(parentId, sourcePath, picked.name));
      homeLoadedFor = "";
      authorsLoadedFor = "";
      await loadDirectory(parentId, folderStack, libraryGeneration, true);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      addingBook = false;
      await picked.dispose();
    }
  }

  async function storeBookImported() {
    homeLoadedFor = "";
    authorsLoadedFor = "";
    folderLoadedFor = "";
  }

  onMount(() => {
    let disposed = false;
    let unlistenDownloadStatus: (() => void) | undefined;
    let unlistenFolderDownloadStatus: (() => void) | undefined;
    const unobserveNetwork = observeNetworkStatus((status) => {
      networkStatus = status;
      void commands.updateNetworkStatus(status).then((reported) => (networkStatus = reported)).catch(() => undefined);
    });
    void loadLibraries();
    void loadAccountStatus();
    void loadBrowsingPreferences();
    void loadReaderPreferences();
    void loadTransferPreferences();
    void listen<DownloadStatusEvent>("library:download-status", ({ payload }) => {
      if (payload.library_id !== activeLibraryId) return;
      const change = payload.event.DownloadStatusChanged;
      bookDownloadStates = { ...bookDownloadStates, [change.book_id]: change.state };
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenDownloadStatus = unlisten;
    }).catch(() => undefined);
    void listen<FolderDownloadStatusEvent>("library:folder-download-status", ({ payload }) => {
      if (payload.library_id !== activeLibraryId) return;
      const change = payload.event.FolderDownloadStatusChanged;
      folderDownloadStatuses = {
        ...folderDownloadStatuses,
        [change.dir_id]: {
          dir_id: change.dir_id,
          downloaded_books: change.downloaded_books,
          total_books: change.total_books,
        },
      };
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenFolderDownloadStatus = unlisten;
    }).catch(() => undefined);
    return () => {
      disposed = true;
      unlistenDownloadStatus?.();
      unlistenFolderDownloadStatus?.();
      unobserveNetwork();
      if (searchTimer) clearTimeout(searchTimer);
      clearThumbnailCache();
    };
  });
</script>

<svelte:head><title>{readerBook ? `${readerBook.title} · Reader` : `${destinationTitle} · ${APP_NAME}`}</title></svelte:head>

{#if readerBook}
  {#key readerBook.key}
    <Reader
      book={readerBook}
      preferences={readerPreferences}
      onpreferenceschange={saveReaderPreference}
      onclose={() => (readerBook = null)}
    />
  {/key}
{:else}
  <div class="adaptive-shell">
    <AdaptiveNavigation active={activeDestination} onnavigate={setDestination} onsettings={() => (showBrowsingSettings = true)} />

    <div class="app-column">
      <AppTopbar
        libraryName={activeLibrary?.library_name ?? "Libraries"}
        folders={folderStack}
        showBreadcrumb={activeDestination === "library"}
        showLibraryActions={activeDestination === "library"}
        {addingBook}
        onlibraries={() => (showLibrarySwitcher = true)}
        onsettings={() => (showBrowsingSettings = true)}
        onaddbook={addBookToCurrentFolder}
        onnewfolder={() => (showCreateFolder = true)}
        onfolder={goToFolder}
      />

      <main class:library-view={activeDestination === "library"} class:authors-view={activeDestination === "authors"} class="content">
        {#if activeDestination === "store"}
          <Store {activeLibraryId} onimported={storeBookImported} />
        {:else if loadingLibrary}
          <section class="page-state" aria-live="polite">
            <span class="loader"></span>
            <strong>Loading your library</strong>
          </section>
        {:else if !libraries.length}
          <section class="first-library">
            <strong>No libraries</strong>
            <p>Add a library to organize your books.</p>
            <form class="first-library-form" onsubmit={createFirstLibrary}>
              <input aria-label="Library name" bind:value={newLibraryName} placeholder="Library name" maxlength="255" />
              <button type="submit" disabled={creatingLibrary}>{creatingLibrary ? "Adding…" : "Add library"}</button>
            </form>
          </section>
        {:else if activeDestination === "home"}
          {#if homeLoading && homeLoadedFor !== activeLibraryId}
            <div class="inline-empty">Loading books…</div>
          {:else}
          {#if recentlyRead.length}
            <BookShelf
              id="recently-read-carousel"
              title="Recently read"
              books={recentlyRead}
              emptyMessage="No books in progress yet."
              first
              {openingBookId}
              downloadStates={bookDownloadStates}
              onopen={openLibraryBook}
              ondownload={downloadBook}
            />
          {/if}

          <BookShelf
            id="recently-added-carousel"
            title="Recently added"
            books={recentlyAdded}
            emptyMessage="No books have been added yet."
            first={!recentlyRead.length}
            maxItems={20}
            {openingBookId}
            downloadStates={bookDownloadStates}
            onopen={openLibraryBook}
            ondownload={downloadBook}
          />
          {/if}
        {:else if activeDestination === "library"}
          <section class="content-section library-section">
            {#if folderLoading}
              <div class="inline-empty">Loading folder…</div>
            {:else}
              {#if directoryContent.dirs.length}
                <div class="folder-grid">
                  {#each directoryContent.dirs.filter((folder) => folder.id !== "__root__") as folder}
                    <FolderCard
                      {folder}
                      status={folderDownloadStatuses[folder.id]}
                      active={activeFolderDownloads[folder.id]}
                      onopen={openFolder}
                      ondownload={downloadFolder}
                    />
                  {/each}
                </div>
              {/if}
              <BookGrid
                books={directoryContent.books}
                emptyMessage={directoryContent.dirs.length ? "" : "This folder is empty."}
                {openingBookId}
                downloadStates={bookDownloadStates}
                onopen={openLibraryBook}
                ondownload={downloadBook}
              />
            {/if}
          </section>
        {:else if activeDestination === "authors"}
          <section class="authors-layout" aria-label="Authors">
            <div class:has-selection={selectedAuthor} class="author-list-panel">
              <label class="author-filter">
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
                <input id="author-filter" type="search" bind:value={authorFilter} onkeydown={handleAuthorFilterKeydown} autocomplete="off" aria-label="Filter authors" />
              </label>
              {#if authorsLoading && authorsLoadedFor !== activeLibraryId}
                <div class="inline-empty">Loading authors…</div>
              {:else if filteredAuthors.length}
                <div class="author-list-scroll">
                  <div class="author-list">
                    {#each visibleAuthors as author, index}
                      <button
                        class:active={selectedAuthor === author.name}
                        type="button"
                        data-author-index={index}
                        tabindex={index === focusedAuthorIndex ? 0 : -1}
                        onfocus={() => (focusedAuthorIndex = index)}
                        onkeydown={(event) => handleAuthorKeydown(event, index)}
                        onclick={() => { focusedAuthorIndex = index; void selectAuthor(author.name); }}
                      >
                        <span class="author-avatar">{author.name.slice(0, 1).toUpperCase() || "?"}</span>
                        <span><strong>{author.name || "Unknown author"}</strong><small>{author.book_count} {author.book_count === 1 ? "book" : "books"}</small></span>
                        <i>›</i>
                      </button>
                    {/each}
                  </div>
                  {#if visibleAuthorCount < filteredAuthors.length}
                    <button class="author-load-more" type="button" onclick={() => (visibleAuthorCount += AUTHOR_PAGE_SIZE)}>
                      Show more ({filteredAuthors.length - visibleAuthorCount})
                    </button>
                  {/if}
                </div>
              {:else}
                <div class="inline-empty">{authorFilter.trim() ? "No matching authors." : "No authors found."}</div>
              {/if}
            </div>

            <div class="author-books-panel">
              {#if authorBooksLoading}
                <div class="inline-empty">Loading books…</div>
              {:else if selectedAuthor}
                <div class="section-heading author-heading">
                  <button class="author-back" type="button" onclick={clearSelectedAuthor}>← <span>Authors</span></button>
                  <div><h2>{selectedAuthor}</h2><p>{authorBooks.length} {authorBooks.length === 1 ? "book" : "books"}</p></div>
                </div>
                <BookGrid books={authorBooks} emptyMessage="No books found for this author." {openingBookId} downloadStates={bookDownloadStates} onopen={openLibraryBook} ondownload={downloadBook} />
              {/if}
            </div>
          </section>
        {:else}
          <section class="search-page">
            <div class="search-intro">
              <h2>Search</h2>
            </div>
            <label class="search-field">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
              <input id="library-search" type="search" bind:value={searchQuery} oninput={scheduleSearch} placeholder="Title or author" autocomplete="off" />
              {#if searchPending}<span class="loader small"></span>{/if}
            </label>
            {#if searchQuery.trim()}
              <div class="search-status">{searchPending ? "Searching…" : `${searchResults.length} ${searchResults.length === 1 ? "result" : "results"}`}</div>
              <BookGrid books={searchResults} emptyMessage={searchPending ? "" : "No matching books."} {openingBookId} downloadStates={bookDownloadStates} onopen={openLibraryBook} ondownload={downloadBook} />
            {/if}
          </section>
        {/if}

        {#if libraryError}
          <p class="error-banner">Library unavailable: {libraryError}</p>
        {/if}
      </main>
    </div>

    {#if showLibrarySwitcher}
      <LibrarySwitcher
        {libraries}
        {activeLibraryId}
        onclose={() => (showLibrarySwitcher = false)}
        onselect={selectLibrary}
        onopenfile={() => { showLibrarySwitcher = false; return openDeviceFile(); }}
      />
    {/if}

    {#if showCreateFolder}
      <CreateFolderDialog onclose={() => (showCreateFolder = false)} oncreate={createFolder} />
    {/if}

    {#if showBrowsingSettings}
      <BrowsingSettingsDialog
        preferences={browsingPreferences}
        account={accountStatus}
        accountBusy={accountBusy}
        accountProgress={accountProgress}
        accountError={accountError}
        {librarySyncStatuses}
        {libraryTransfers}
        {librarySyncBusyId}
        {networkStatus}
        {transferPreferences}
        onclose={() => (showBrowsingSettings = false)}
        onchange={saveBrowsingPreference}
        onlogin={loginToServer}
        onsync={syncServerLibraries}
        onsynclibrary={syncLibraryToServer}
        ontransferpreference={saveTransferPreference}
        onlogout={logoutFromServer}
      />
    {/if}
  </div>
{/if}

<style>
  :global(:root) {
    font-family: var(--font-interface-family);
    color: var(--color-ink-text);
    background: var(--color-surface-bg);
    font-synthesis: none;
    text-rendering: optimizeLegibility;
    -webkit-font-smoothing: antialiased;
    -webkit-text-size-adjust: 100%;
  }

  :global(*) { box-sizing: border-box; }
  :global(html), :global(body), :global(#app) { height: 100%; }
  :global(body) { margin: 0; min-width: 20rem; overflow: hidden; }
  button, input { font: inherit; }
  button, label { -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }

  .adaptive-shell {
    --page-gutter: 1rem;
    height: 100dvh;
    min-height: 100dvh;
    overflow: hidden;
    background:
      radial-gradient(circle at 92% -8rem, color-mix(in srgb, var(--color-accent) 15%, transparent), transparent 28rem),
      var(--color-surface-bg);
  }

  .app-column { display: flex; flex-direction: column; min-width: 0; min-height: 0; height: 100dvh; }

  .section-heading p {
    margin: 0 0 .18rem;
    color: var(--color-accent);
    font-size: .64rem;
    font-weight: 800;
    letter-spacing: .12em;
    text-transform: uppercase;
  }

  .content {
    flex: 1;
    min-height: 0;
    width: 100%;
    max-width: 92rem;
    margin: 0 auto;
    padding: 1rem var(--page-gutter) calc(6.7rem + env(safe-area-inset-bottom));
    padding-right: max(var(--page-gutter), env(safe-area-inset-right));
    padding-left: max(var(--page-gutter), env(safe-area-inset-left));
    overflow-y: scroll;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    scrollbar-color: var(--color-disabled-text) transparent;
    scrollbar-width: thin;
  }
  .content::-webkit-scrollbar { width: .55rem; }
  .content::-webkit-scrollbar-track { background: transparent; }
  .content::-webkit-scrollbar-thumb { border: .14rem solid transparent; border-radius: 999px; background: var(--color-disabled-text); background-clip: padding-box; }
  .content.library-view { padding-top: 0; }

  .section-heading h2, .search-intro h2 {
    margin: 0;
    font-family: var(--font-heading-family);
    font-weight: 500;
    letter-spacing: -.035em;
  }
  .content-section { margin-top: 2rem; }
  .section-heading { display: flex; align-items: end; justify-content: space-between; gap: 1rem; margin-bottom: .9rem; }
  .section-heading h2, .search-intro h2 { font-size: clamp(1.55rem, 7vw, 2.15rem); }

  .page-state, .first-library {
    display: grid;
    place-items: center;
    min-height: 17rem;
    padding: 2rem;
    text-align: center;
    color: var(--color-subtle-text);
  }
  .page-state { gap: .75rem; }
  .loader { width: 1.4rem; height: 1.4rem; border: .18rem solid var(--color-border-color); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  .loader.small { width: 1rem; height: 1rem; border-width: .13rem; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .inline-empty { display: grid; place-items: center; min-height: 8rem; color: var(--color-subtle-text); font-size: .84rem; text-align: center; }

  .library-section { margin-top: 0; }
  .folder-grid { display: flex; flex-wrap: wrap; align-items: flex-start; gap: .45rem; margin-bottom: 1.25rem; }

  .content.authors-view { display: flex; flex-direction: column; overflow: hidden; }
  .authors-layout { display: grid; flex: 1; min-height: 0; gap: 2rem; overflow: hidden; }
  .author-list-panel { display: flex; flex-direction: column; min-height: 0; }
  .author-list-panel.has-selection { display: none; }
  .author-books-panel { display: none; min-height: 0; overflow-y: auto; }
  .author-list-panel.has-selection + .author-books-panel { display: block; }
  .author-heading { align-items: start; justify-content: start; }
  .author-back { display: inline-flex; align-items: center; gap: .35rem; padding: .45rem .55rem; border: 1px solid var(--color-border-color); border-radius: .65rem; color: var(--color-accent); background: color-mix(in srgb, var(--color-white-surface) 58%, transparent); font-size: .72rem; font-weight: 750; }
  .author-filter { display: grid; flex: none; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: .6rem; min-height: 2.9rem; margin-bottom: .8rem; padding: 0 .75rem; border: 1px solid var(--color-border-color); border-radius: .65rem; background: var(--color-white-surface); }
  .author-filter:focus-within { border-color: var(--color-accent); }
  .author-filter svg { width: 1.05rem; color: var(--color-accent); }
  .author-filter input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font: inherit; font-size: .82rem; }
  .author-list-scroll { flex: 1; min-height: 0; padding-right: .2rem; overflow-y: auto; overscroll-behavior-y: contain; scrollbar-color: var(--color-disabled-text) transparent; scrollbar-width: thin; }
  .author-list { display: grid; gap: .4rem; }
  .author-load-more { display: block; min-height: 2.5rem; margin: .8rem auto 0; padding: .5rem .9rem; border: 1px solid var(--color-border-color); border-radius: .6rem; color: var(--color-accent); background: var(--color-white-surface); font-size: .72rem; font-weight: 750; cursor: pointer; }
  .author-list button {
    display: grid;
    grid-template-columns: 2.65rem minmax(0, 1fr) auto;
    align-items: center;
    gap: .7rem;
    width: 100%;
    min-height: 3.65rem;
    padding: .5rem .65rem;
    border: 1px solid transparent;
    border-radius: .85rem;
    color: inherit;
    text-align: left;
    background: transparent;
  }
  .author-list button.active { border-color: var(--color-border-color); background: color-mix(in srgb, var(--color-white-surface) 60%, transparent); }
  .author-list button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .author-list button > span:nth-child(2) { display: grid; min-width: 0; gap: .12rem; }
  .author-list strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: 500 .95rem var(--font-heading-family); }
  .author-list small { color: var(--color-subtle-text); font-size: .68rem; }
  .author-list i { color: var(--color-disabled-text); font-style: normal; }
  .author-avatar { display: grid; place-items: center; width: 2.65rem; aspect-ratio: 1; border-radius: 50%; color: var(--color-white-surface); background: var(--color-accent); font: 500 1rem var(--font-heading-family); }

  .search-intro { margin: 1rem 0 1.2rem; }
  .search-field { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: .7rem; min-height: 3.6rem; padding: 0 1rem; border: 1px solid var(--color-border-color); border-radius: 1rem; background: color-mix(in srgb, var(--color-white-surface) 70%, transparent); box-shadow: 0 .7rem 2rem color-mix(in srgb, var(--color-box-shadow) 20%, transparent); }
  .search-field svg { width: 1.25rem; color: var(--color-accent); }
  .search-field input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font-size: 1rem; }
  .search-status { min-height: 2.8rem; padding: 1rem .15rem .5rem; color: var(--color-subtle-text); font-size: .74rem; }

  .first-library { min-height: 19rem; align-content: center; gap: .5rem; border: 1px dashed var(--color-border-color); border-radius: 1.25rem; }
  .first-library strong { color: var(--color-ink-text); font: 500 1.35rem var(--font-heading-family); }
  .first-library p { margin: 0 0 .6rem; font-size: .82rem; }
  .first-library-form { display: flex; width: min(100%, 23rem); gap: .5rem; }
  .first-library-form input { min-width: 0; flex: 1; min-height: 2.75rem; padding: .65rem .8rem; border: 1px solid var(--color-border-color); border-radius: .6rem; color: var(--color-ink-text); background: var(--color-card-bg); }
  .first-library-form button { min-height: 2.75rem; padding: .65rem 1rem; border: 0; border-radius: .6rem; color: var(--color-white-surface); background: var(--color-accent); font-size: .8rem; font-weight: 750; cursor: pointer; }
  .first-library-form button:disabled { opacity: .6; cursor: default; }
  .error-banner { margin: 1rem 0; padding: .75rem 1rem; border-radius: .75rem; color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .75rem; overflow-wrap: anywhere; }

  @media (min-width: 45rem) {
    .adaptive-shell { --page-gutter: 1.5rem; display: grid; grid-template-columns: 5.25rem minmax(0, 1fr); }
    .content { padding-top: 1.5rem; padding-right: max(var(--page-gutter), env(safe-area-inset-right)); padding-bottom: 3rem; padding-left: max(var(--page-gutter), env(safe-area-inset-left)); }
    .content.authors-view { padding-bottom: 1.5rem; }
    .authors-layout { grid-template-columns: minmax(15rem, 20rem) minmax(0, 1fr); align-items: stretch; }
    .author-list-panel.has-selection { display: flex; }
    .author-books-panel { display: block; }
    .author-back { display: none; }
    .author-list-panel { position: static; max-height: none; overflow: hidden; }
  }

  @media (min-width: 70rem) {
    .adaptive-shell { grid-template-columns: 14.5rem minmax(0, 1fr); }
  }

  @media (min-width: 88rem) {
    .adaptive-shell { --page-gutter: 2rem; grid-template-columns: 15.5rem minmax(0, 1fr); }
  }

  @media (prefers-reduced-motion: reduce) {
    .loader { animation: none; }
  }

  /* Compact library UI, aligned with the Floem desktop application. */
  .adaptive-shell { background: var(--color-app-bg); }
  .content { max-width: 78rem; padding-top: 1.1rem; }
  .content-section { margin-top: 1.8rem; }
  .section-heading { margin-bottom: .8rem; }
  .section-heading h2, .search-intro h2 { font-size: 1.35rem; letter-spacing: -.02em; }
  .inline-empty { min-height: 5rem; justify-items: start; text-align: left; }
  .first-library { min-height: 12rem; align-content: center; border: 0; border-radius: 0; }
  .first-library strong { font-family: inherit; font-size: 1rem; }
  .search-intro { margin-top: 0; }
  .search-field { border-radius: .65rem; box-shadow: none; }
  .author-list button { border-radius: .55rem; }
  .author-list button.active { background: var(--color-active-highlight); }

  @media (min-width: 88rem) {
    .adaptive-shell { grid-template-columns: 15.5rem minmax(0, 1fr); }
  }
</style>
