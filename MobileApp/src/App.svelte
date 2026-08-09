<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { listen } from "./lib/app-events";

  import Reader from "./components/Reader.svelte";
  import LoginScreen from "./components/auth/LoginScreen.svelte";
  import BookGrid from "./components/library/BookGrid.svelte";
  import BookShelf from "./components/library/BookShelf.svelte";
  import AuthorCatalogResults from "./components/library/AuthorCatalogResults.svelte";
  import CreateFolderDialog from "./components/library/CreateFolderDialog.svelte";
  import CreateLibraryDialog from "./components/library/CreateLibraryDialog.svelte";
  import FolderCard from "./components/library/FolderCard.svelte";
  import AdaptiveNavigation from "./components/shell/AdaptiveNavigation.svelte";
  import AppTopbar from "./components/shell/AppTopbar.svelte";
  import BrowsingSettingsDialog from "./components/shell/BrowsingSettingsDialog.svelte";
  import EmptyPage from "./components/shell/EmptyPage.svelte";
  import LibrarySwitcher from "./components/shell/LibrarySwitcher.svelte";
  import ServerAccountDialog from "./components/shell/ServerAccountDialog.svelte";
  import Store from "./components/store/Store.svelte";
  import { applyApplicationAppearance } from "./lib/application-appearance";
  import { commands } from "./lib/commands";
  import { pickEpub, type PickedEpub } from "./lib/epub-picker";
  import { NAV_ITEMS, type Destination } from "./lib/navigation";
  import { observeNetworkStatus, UNKNOWN_NETWORK_STATUS } from "./lib/network-status";
  import { commitAndMeasureRender, measureInvoke } from "./lib/performance";
  import { clearThumbnailCache } from "./lib/thumbnail-cache";
  import { readUiSession, writeUiSession, type AppUiSession } from "./lib/ui-session";
  import {
    BROWSING_CONTENT_CONTRACT,
    BROWSING_LAYOUT_CONTRACT,
    BROWSING_STATES_CONTRACT,
    DEFAULT_READER_PREFERENCES,
    DEFAULT_BROWSING_PREFERENCES,
    type AccountSessionSummary,
    type AccountStatus,
    type AuthorSummary,
    type BookCardData,
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
  import { isWebApp } from "./lib/platform";

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
  type LibraryViewMode = "cover" | "detailed";

  const EMPTY_DIRECTORY: DirectoryContent = { books: [], directories: [] };
  const SEARCH_DEBOUNCE_MS = BROWSING_CONTENT_CONTRACT.search_debounce_ms;
  const naturalCollator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
  const WEB_PREFERENCES_KEY = `${APP_NAME_LOWER}-reader-preferences`;
  const BROWSING_UI_PREFERENCES_KEY = `${APP_NAME_LOWER}-browsing-ui-preferences`;
  const DEFAULT_LIBRARY_VIEW_MODE: LibraryViewMode = "cover";

  function currentSessionFromWebRoute(): AppUiSession | null {
    const libraryId = (page.url.searchParams.get("library") || page.params.libraryId || undefined)?.trim() || undefined;
    const routeId = page.route.id;
    if (routeId === "/reader/[bookId]" && page.params.bookId && page.params.bookId.trim()) {
      return {
        kind: "reader",
        book_id: page.params.bookId,
        library_id: libraryId,
      };
    }
    if (routeId === "/library/[libraryId]/folder/[folderId]") {
      const folderId = page.params.folderId?.trim() || "__root__";
      return {
        kind: "browse",
        destination: "library",
        library_id: page.params.libraryId || libraryId,
        folder_id: folderId === "__root__" ? "__root__" : folderId,
      };
    }
    if (routeId === "/library/[libraryId]/authors") {
      return {
        kind: "browse",
        destination: "authors",
        library_id: page.params.libraryId || libraryId,
      };
    }
    if (routeId === "/library/[libraryId]/search") {
      return {
        kind: "browse",
        destination: "search",
        library_id: page.params.libraryId || libraryId,
      };
    }
    if (routeId === "/store") {
      return { kind: "browse", destination: "store" };
    }
    if (routeId === "/account") {
      return { kind: "browse", destination: "account" };
    }
    if (routeId === "/settings") {
      return { kind: "browse", destination: "settings" };
    }
    if (routeId === "/library" || routeId === "/" || routeId === "/login" || routeId === null) {
      return {
        kind: "browse",
        destination: "home",
        library_id: libraryId,
      };
    }
    return {
      kind: "browse",
      destination: activeDestination || "home",
      library_id: activeLibraryId || libraryId,
      folder_id: folderStack.at(-1)?.id || "__root__",
    };
  }

  function currentSessionFromRuntimeState(): AppUiSession {
    if (readerBook?.bookId && !readerBook.bookId.startsWith("foliate-position:")) {
      return {
        kind: "reader",
        book_id: readerBook.bookId,
        library_id: activeLibraryId || undefined,
      };
    }
    return {
      kind: "browse",
      destination: activeDestination,
      library_id: activeLibraryId || undefined,
      folder_id: activeDestination === "library" ? (folderStack.at(-1)?.id || "__root__") : undefined,
    };
  }

  function naturalCompare(left: string, right: string) {
    return naturalCollator.compare(left, right);
  }

  function sortedLibraries(entries: LibraryEntry[]) {
    return [...entries].sort((left, right) => naturalCompare(left.library_name, right.library_name));
  }
  const browsingLayoutStyle = [
    `--layout-sidebar-width: ${BROWSING_LAYOUT_CONTRACT.sidebar_width_px}px`,
    `--layout-topbar-height: ${BROWSING_LAYOUT_CONTRACT.topbar_height_px}px`,
    `--layout-topbar-action-height: ${BROWSING_LAYOUT_CONTRACT.topbar_action_height_px}px`,
    `--layout-topbar-action-gap: ${BROWSING_LAYOUT_CONTRACT.topbar_action_gap_px}px`,
    `--layout-topbar-action-horizontal-padding: ${BROWSING_LAYOUT_CONTRACT.topbar_action_horizontal_padding_px}px`,
    `--layout-content-padding: ${BROWSING_LAYOUT_CONTRACT.content_padding_px}px`,
    `--book-cover-width: ${BROWSING_LAYOUT_CONTRACT.book_cover_width_px}px`,
    `--book-cover-height: ${BROWSING_LAYOUT_CONTRACT.book_cover_height_px}px`,
    `--book-card-height: ${BROWSING_LAYOUT_CONTRACT.book_card_height_px}px`,
    `--book-grid-column-gap: ${BROWSING_LAYOUT_CONTRACT.book_grid_column_gap_px}px`,
    `--book-grid-row-height: ${BROWSING_LAYOUT_CONTRACT.book_grid_row_height_px}px`,
  ].join("; ");

  let libraries = $state<LibraryEntry[]>([]);
  let activeLibraryId = $state("");
  let activeDestination = $state<Destination>("home");
  let recentlyAdded = $state<BookCardData[]>([]);
  let recentlyRead = $state<BookCardData[]>([]);
  const recentlyInProgress = $derived(recentlyRead.filter((book) => (book.progress ?? 0) < BROWSING_CONTENT_CONTRACT.finished_progress_threshold));
  const recentlyFinished = $derived(recentlyRead.filter((book) => (book.progress ?? 0) >= BROWSING_CONTENT_CONTRACT.finished_progress_threshold));
  let directoryContent = $state<DirectoryContent>(EMPTY_DIRECTORY);
  let folderStack = $state<FolderLevel[]>([]);
  let authors = $state<AuthorSummary[]>([]);
  let authorFilter = $state("");
  let focusedAuthorIndex = $state(0);
  let selectedAuthorId = $state("");
  let selectedAuthorName = $state("");
  let authorBooks = $state<BookCardData[]>([]);
  let searchQuery = $state("");
  let searchResults = $state<BookCardData[]>([]);
  let searchCompletedQuery = $state("");
  let searchPending = $state(false);
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let showLibrarySwitcher = $state(false);
  let showCreateFolder = $state(false);
  let showCreateLibrary = $state(false);
  let readerBook = $state<ReaderBook | null>(null);
  let loadingLibrary = $state(true);
  let homeLoading = $state(false);
  let folderLoading = $state(false);
  let authorsLoading = $state(false);
  let authorBooksLoading = $state(false);
  let homeLoadedFor = $state("");
  let folderLoadedFor = $state("");
  let authorsLoadedFor = $state("");
  let libraryGeneration = 0;
  let folderRequest = 0;
  let folderPendingKey = "";
  let authorRequest = 0;
  let openingBookId = $state("");
  let pendingOpenBook: BookCardData | null = null;
  let bookDownloadStates = $state<Record<string, DownloadState>>({});
  let folderDownloadStatuses = $state<Record<string, FolderDownloadStatus>>({});
  let activeFolderDownloads = $state<Record<string, boolean>>({});
  let libraryError = $state("");
  let creatingLibrary = $state(false);
  let addingBook = $state(false);
  let browsingPreferences = $state<BrowsingPreferences>({ ...DEFAULT_BROWSING_PREFERENCES });
  let readerPreferences = $state<ReaderPreferences>({ ...DEFAULT_READER_PREFERENCES });
  let libraryViewMode = $state<LibraryViewMode>(DEFAULT_LIBRARY_VIEW_MODE);
  let accountStatus = $state<AccountStatus>({ signed_in: false, server_url: null, email: null });
  let accountBusy = $state(false);
  let accountProgress = $state("");
  let accountError = $state("");
  let settingsError = $state("");
  let accountResolved = $state(!isWebApp);
  let webAppReady = $state(!isWebApp);
  let librarySyncStatuses = $state<LibrarySyncStatus[]>([]);
  let libraryTransfers = $state<LibraryTransfers[]>([]);
  let accountSessions = $state<AccountSessionSummary[]>([]);
  let networkStatus = $state<NetworkStatus>({ ...UNKNOWN_NETWORK_STATUS });
  let transferPreferences = $state<TransferPreferences>({ allow_background_on_metered: false, auto_download_limit_mb: null });
  let browsingPreferenceSaveQueue = Promise.resolve();
  let browsingPreferenceVersion = 0;
  let preferenceSaveQueue = Promise.resolve();
  let preferenceVersion = 0;
  let restoringReaderId = "";
  let readerReturnPath = "";
  const readerRouteCache = new Map<string, ReaderBook>();

  const destinationTitle = $derived(activeDestination === "account" ? "Account" : activeDestination === "settings" ? "Settings" : NAV_ITEMS.find((item) => item.id === activeDestination)?.label ?? APP_NAME);
  const activeLibrary = $derived(libraries.find((library) => library.library_id === activeLibraryId));
  const visibleDirectoryBooks = $derived(directoryContent.books);
  const visibleAuthorBooks = $derived(authorBooks);
  const visibleSearchResults = $derived(searchResults);
  const visibleRecentlyInProgress = $derived(recentlyInProgress);
  const visibleRecentlyFinished = $derived(recentlyFinished);
  const visibleRecentlyAdded = $derived(recentlyAdded);
  const filteredAuthors = $derived(authors.filter((author) => author.name.toLocaleLowerCase().includes(authorFilter.trim().toLocaleLowerCase())));
  const visibleAuthors = $derived(filteredAuthors);

  function libraryRoute(destination: Destination = activeDestination, libraryId = activeLibraryId, folderId?: string) {
    if (destination === "account") return "/account";
    if (destination === "settings") return "/settings";
    if (destination === "store") return "/store";
    if (!libraryId) return "/library";
    const library = `/library/${encodeURIComponent(libraryId)}`;
    if (destination === "library") return folderId && folderId !== "__root__" ? `${library}/folder/${encodeURIComponent(folderId)}` : `${library}/folder/__root__`;
    if (destination === "authors") return `${library}/authors`;
    if (destination === "search") return `${library}/search`;
    return library;
  }

  function navigate(path: string, replaceState = false) {
    if (isWebApp && `${page.url.pathname}${page.url.search}` !== path) void goto(path, { replaceState, noScroll: true, keepFocus: true });
  }

  function closeReader() {
    if (isWebApp && page.route.id === "/reader/[bookId]") {
      const destination = readerReturnPath || libraryRoute("home", activeLibraryId);
      readerBook = null;
      navigate(destination, true);
    } else {
      readerBook = null;
    }
  }

  function navigateToReader(bookId: string, book: ReaderBook) {
    if (!isWebApp) return;
    if (page.route.id !== "/reader/[bookId]") {
      readerReturnPath = `${page.url.pathname}${page.url.search}`;
    }
    readerRouteCache.set(bookId, book);
    const library = activeLibraryId ? `?library=${encodeURIComponent(activeLibraryId)}` : "";
    navigate(`/reader/${encodeURIComponent(bookId)}${library}`);
  }

  $effect(() => {
    applyApplicationAppearance(browsingPreferences);
  });

  $effect(() => {
    authorFilter;
    focusedAuthorIndex = 0;
  });

  $effect(() => {
    if (activeDestination !== "account" || !accountStatus.signed_in) return;
    void Promise.all([loadServerTransferState(), loadAccountSessions()]).catch((reason) => (accountError = errorMessage(reason)));
  });

  $effect(() => {
    if (!isWebApp) return;
    const route = {
      id: page.route.id,
      pathname: page.url.pathname,
      libraryId: page.params.libraryId,
      folderId: page.params.folderId,
      bookId: page.params.bookId,
      readerLibraryId: page.url.searchParams.get("library") ?? undefined,
    };
    void synchronizeWebRoute(route);
  });

  $effect(() => {
    const session = buildSessionForPersistence();
    writeUiSession(session);
  });

  function errorMessage(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  async function synchronizeWebRoute(route: {
    id: string | null;
    pathname: string;
    libraryId?: string;
    folderId?: string;
    bookId?: string;
    readerLibraryId?: string;
  }) {
    if (!accountResolved) return;
    if (!accountStatus.signed_in) {
      readerBook = null;
      if (route.id !== "/login") navigate("/login", true);
      return;
    }
    if (!webAppReady) return;

    if (route.id === "/" || route.id === "/login" || route.id === "/library") {
      navigate(libraryRoute("home"), true);
      return;
    }

    if (route.id === "/reader/[bookId]" && route.bookId) {
      if (route.readerLibraryId && route.readerLibraryId !== activeLibraryId && libraries.some((entry) => entry.library_id === route.readerLibraryId)) {
        await selectLibrary(route.readerLibraryId, false);
      }
      await restoreReaderRoute(route.bookId);
      return;
    }

    if (readerBook) readerBook = null;
    if (route.id === "/store") {
      setDestination("store", false);
      return;
    }

    if (route.id === "/account" || route.id === "/settings") {
      setDestination(route.id === "/account" ? "account" : "settings", false);
      return;
    }

    const destination: Destination = route.id === "/library/[libraryId]/authors"
      ? "authors"
      : route.id === "/library/[libraryId]/search"
        ? "search"
        : route.id === "/library/[libraryId]/folder/[folderId]"
          ? "library"
          : "home";

    if (route.libraryId && route.libraryId !== activeLibraryId && libraries.some((entry) => entry.library_id === route.libraryId)) {
      await selectLibrary(route.libraryId, false);
    }
    if (!activeLibraryId) return;
    activeDestination = destination;
    libraryError = "";
    if (destination === "library") {
      const folderId = route.folderId ?? "__root__";
      const stack = folderId === "__root__"
        ? []
        : folderStack.at(-1)?.id === folderId
          ? folderStack
          : [{ id: folderId, name: directoryContent.directories.find((folder) => folder.id === folderId)?.name ?? "Folder" }];
      await loadDirectory(folderId, stack);
    } else {
      await ensureDestination(destination);
    }
    if (destination === "search") {
      requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#library-search")?.focus());
    }
  }

  function buildSessionForPersistence(): AppUiSession | null {
    if (isWebApp) {
      const session = currentSessionFromWebRoute();
      if (!session) return null;
      if (session.kind === "reader" && session.book_id.startsWith("foliate-position:")) return null;
      return session;
    }
    const session = currentSessionFromRuntimeState();
    if (session.kind === "reader" && session.book_id.startsWith("foliate-position:")) return null;
    return session;
  }

  function routeFromSession(session: AppUiSession): string {
    if (session.kind === "reader") {
      const library = session.library_id ? `?library=${encodeURIComponent(session.library_id)}` : "";
      return `/reader/${encodeURIComponent(session.book_id)}${library}`;
    }
    if (session.destination === "account") return "/account";
    if (session.destination === "settings") return "/settings";
    if (session.destination === "store") return "/store";
    if (session.destination === "library") {
      return libraryRoute("library", session.library_id, session.folder_id);
    }
    if (session.destination === "authors") {
      return libraryRoute("authors", session.library_id);
    }
    if (session.destination === "search") {
      return libraryRoute("search", session.library_id);
    }
    return libraryRoute("home", session.library_id);
  }

  async function selectSessionLibrary(libraryId: string | undefined): Promise<boolean> {
    if (!libraryId) return Boolean(activeLibraryId);
    if (libraryId === activeLibraryId) return true;
    if (!libraries.some((entry) => entry.library_id === libraryId)) return false;
    await selectLibrary(libraryId, false);
    return activeLibraryId === libraryId;
  }

  async function restoreUiSession(session: AppUiSession | null) {
    if (!session) return;
    if (isWebApp) {
      const current = `${page.url.pathname}${page.url.search}`;
      if (page.route.id !== "/" && page.route.id !== "/library" && page.route.id !== "/login" && page.route.id !== null) return;
      const route = routeFromSession(session);
      if (route && route !== current) {
        navigate(route, true);
      }
      return;
    }

    if (session.kind === "reader") {
      const restored = await selectSessionLibrary(session.library_id);
      if (restored) {
        await restoreReaderRoute(session.book_id);
      } else {
        const fallback: AppUiSession = { kind: "browse", destination: "home" };
        await restoreReaderSession(fallback);
      }
      return;
    }
    await restoreReaderSession(session);
  }

  async function restoreReaderSession(session: AppUiSession) {
    if (session.kind === "reader") {
      if (!session.book_id) return;
      if (!(await selectSessionLibrary(session.library_id))) return;
      await restoreReaderRoute(session.book_id);
      return;
    }
    if (session.destination === "account" || session.destination === "settings" || session.destination === "store") {
      activeDestination = session.destination;
      libraryError = "";
      return;
    }
    if (session.destination === "home") {
      activeDestination = "home";
      libraryError = "";
      if (activeLibraryId) await ensureDestination("home");
      return;
    }
    if (session.destination === "authors") {
      activeDestination = "authors";
      libraryError = "";
      if (await selectSessionLibrary(session.library_id)) await ensureDestination("authors");
      return;
    }
    if (session.destination === "search") {
      activeDestination = "search";
      libraryError = "";
      if (await selectSessionLibrary(session.library_id)) {
        await ensureDestination("search");
        requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#library-search")?.focus());
      }
      return;
    }
    await selectSessionLibrary(session.library_id);
    activeDestination = "library";
    const folderId = session.folder_id && session.folder_id !== "__root__" ? session.folder_id : "__root__";
    if (folderId === "__root__") {
      if (activeLibraryId) await loadDirectory("__root__", []);
    } else {
      await loadDirectory(folderId, [{ id: folderId, name: "" }]);
    }
    if (activeDestination !== "library") return;
  }

  function setDestination(destination: Destination, updateRoute = true) {
    activeDestination = destination;
    libraryError = "";
    void ensureDestination(destination);
    if (updateRoute) navigate(libraryRoute(destination, activeLibraryId, folderStack.at(-1)?.id));
    if (destination === "search") {
      requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#library-search")?.focus());
    }
    if (destination === "account" && accountStatus.signed_in) {
      void loadServerTransferState().catch((reason) => (accountError = errorMessage(reason)));
    }
  }

  function openLibrarySwitcher() {
    showLibrarySwitcher = true;
  }

  async function retryActiveSection() {
    libraryError = "";
    if (pendingOpenBook) {
      await openLibraryBook(pendingOpenBook);
      return;
    }
    if (!libraries.length) {
      await loadLibraries();
      return;
    }
    if (activeDestination === "home") await loadHome(libraryGeneration, true);
    if (activeDestination === "library") await loadDirectory(folderStack.at(-1)?.id ?? "__root__", folderStack, libraryGeneration, true);
    if (activeDestination === "authors") await loadAuthors(libraryGeneration, true);
    if (activeDestination === "account") await loadServerTransferState();
  }

  function resetLibraryData() {
    folderRequest += 1;
    folderPendingKey = "";
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
    selectedAuthorId = "";
    selectedAuthorName = "";
    authorBooks = [];
    folderStack = [];
    searchQuery = "";
    searchResults = [];
    homeLoadedFor = "";
    folderLoadedFor = "";
    authorsLoadedFor = "";
    bookDownloadStates = {};
    folderDownloadStatuses = {};
    activeFolderDownloads = {};
    pendingOpenBook = null;
  }

  function mergeBookDownloadStates(books: BookCardData[]) {
    const next = { ...bookDownloadStates };
    for (const book of books) next[book.book_id] = book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded";
    bookDownloadStates = next;
  }

  function replaceFolderDownloadStates(folders: LibraryDirectory[]) {
    folderDownloadStatuses = Object.fromEntries(
      folders
        .filter((folder) => folder.id !== "__root__")
        .map((folder) => [folder.id, { dir_id: folder.id, downloaded_books: folder.downloaded_book_count, total_books: folder.book_count }]),
    );
  }

  async function downloadBook(book: BookCardData) {
    if (bookDownloadStates[book.book_id] === "Downloaded") return Promise.resolve();
    bookDownloadStates = { ...bookDownloadStates, [book.book_id]: "Queued" };
    try {
      const state = await commands.downloadBook(activeLibraryId, book.book_id);
      if (bookDownloadStates[book.book_id] !== "Downloaded") bookDownloadStates = { ...bookDownloadStates, [book.book_id]: state };
    } catch (reason) {
      libraryError = errorMessage(reason);
      throw reason;
    }
  }

  async function removeLocalBookCopy(book: BookCardData) {
    if (!window.confirm(`Remove the downloaded copy of “${book.title || "Untitled"}” from this device? You can download it again later.`)) return;
    libraryError = "";
    try {
      const state = await commands.removeLocalBookCopy(activeLibraryId, book.book_id);
      bookDownloadStates = { ...bookDownloadStates, [book.book_id]: state };
    } catch (reason) {
      libraryError = errorMessage(reason);
    }
  }

  async function downloadFolder(folder: LibraryDirectory) {
    if (activeFolderDownloads[folder.id]) return;
    activeFolderDownloads = { ...activeFolderDownloads, [folder.id]: true };
    libraryError = "";
    try {
      const status = await commands.downloadFolder(activeLibraryId, folder.id);
      folderDownloadStatuses = { ...folderDownloadStatuses, [folder.id]: status };
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      activeFolderDownloads = { ...activeFolderDownloads, [folder.id]: false };
    }
  }

  function clearSelectedAuthor() {
    authorRequest += 1;
    selectedAuthorId = "";
    selectedAuthorName = "";
    authorBooks = [];
    authorBooksLoading = false;
    requestAnimationFrame(() => document.querySelector<HTMLButtonElement>(`[data-author-index="${focusedAuthorIndex}"]`)?.focus());
  }

  function focusAuthor(index: number) {
    const lastVisibleIndex = filteredAuthors.length - 1;
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
      const home = await measureInvoke("home", () => commands.getLibraryHome(libraryId, BROWSING_CONTENT_CONTRACT.home_book_limit));
      if (generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      await commitAndMeasureRender("home", () => {
        recentlyAdded = home.recently_added;
        recentlyRead = home.recently_read;
        homeLoadedFor = libraryId;
      }, home.recently_added.length + home.recently_read.length);
      mergeBookDownloadStates([...home.recently_added, ...home.recently_read]);
    } catch (reason) {
      if (generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (generation === libraryGeneration) homeLoading = false;
    }
  }

  async function loadDirectory(dirId: string, nextStack: FolderLevel[], generation = libraryGeneration, force = false) {
    const libraryId = activeLibraryId;
    const key = `${libraryId}:${dirId}`;
    if (!libraryId || (!force && folderLoadedFor === key) || (!force && folderPendingKey === key)) return;
    const request = ++folderRequest;
    folderPendingKey = key;
    folderLoading = true;
    libraryError = "";
    try {
      const content = await measureInvoke("directory", () => commands.getLibraryFolder(libraryId, dirId));
      if (request !== folderRequest || generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      const sortedContent = {
        ...content,
        directories: [...content.directories].sort((left, right) => naturalCompare(left.name, right.name)),
        books: [...content.books].sort((left, right) => naturalCompare(left.title, right.title)),
      };
      await commitAndMeasureRender("directory", () => {
        directoryContent = sortedContent;
        folderStack = nextStack;
        folderLoadedFor = key;
      }, content.books.length + content.directories.length);
      mergeBookDownloadStates(content.books);
      replaceFolderDownloadStates(content.directories);
    } catch (reason) {
      if (request === folderRequest && generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (request === folderRequest && generation === libraryGeneration) {
        folderPendingKey = "";
        folderLoading = false;
      }
    }
  }

  async function loadAuthors(generation: number, force = false) {
    const libraryId = activeLibraryId;
    if (!libraryId || authorsLoading || (!force && authorsLoadedFor === libraryId)) return;
    authorsLoading = true;
    try {
      const view = await measureInvoke("authors", () => commands.getLibraryAuthors(libraryId, null));
      if (generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      await commitAndMeasureRender("authors", () => {
        authors = view.authors;
        authorsLoadedFor = libraryId;
      }, view.authors.length);
    } catch (reason) {
      if (generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (generation === libraryGeneration) authorsLoading = false;
    }
  }

  async function selectLibrary(libraryId: string, updateRoute = true) {
    const generation = ++libraryGeneration;
    showLibrarySwitcher = false;
    showCreateFolder = false;
    activeLibraryId = libraryId;
    activeDestination = "home";
    libraryError = "";
    loadingLibrary = true;
    resetLibraryData();
    try {
      loadingLibrary = false;
      await ensureDestination("home", generation);
      if (updateRoute) navigate(libraryRoute("home", libraryId));
    } catch (reason) {
      if (generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (generation === libraryGeneration) loadingLibrary = false;
    }
  }

  async function loadLibraries() {
    try {
      const entries = await measureInvoke("libraries", () => commands.getLibraries());
      const sortedEntries = sortedLibraries(entries);
      await commitAndMeasureRender("libraries", () => (libraries = sortedEntries), entries.length);
      const routeLibraryId = page.params.libraryId ?? page.url.searchParams.get("library");
      const initialLibrary = sortedEntries.find((entry) => entry.library_id === routeLibraryId) ?? sortedEntries[0];
      if (initialLibrary) await selectLibrary(initialLibrary.library_id, false);
      else loadingLibrary = false;
    } catch (reason) {
      // The standalone browser build still supports opening a local EPUB.
      libraryError = errorMessage(reason);
      loadingLibrary = false;
    }
  }

  async function loadAccountStatus() {
    try {
      const status = await commands.getAccountStatus();
      accountStatus = status;
    } catch (reason) {
      accountStatus = { signed_in: false, server_url: null, email: null };
      if (isWebApp) accountError = `Could not check your session: ${errorMessage(reason)}`;
    } finally {
      accountResolved = true;
    }
  }

  async function prepareAuthenticatedWebApp() {
    webAppReady = false;
    const enrollment = await Promise.allSettled([commands.bootstrapRemoteLibraries()]);
    const results = [...enrollment, ...await Promise.allSettled([
      loadServerTransferState(),
      loadBrowsingPreferences(),
      loadReaderPreferences(),
      loadTransferPreferences(),
    ])];
    const failed = results.find((result) => result.status === "rejected");
    if (failed?.status === "rejected") libraryError = errorMessage(failed.reason);
    clearThumbnailCache();
    await loadLibraries();
    try {
      networkStatus = await commands.updateNetworkStatus(networkStatus);
    } catch {
      // Network reporting is advisory and must not prevent the application opening.
    }
    await restoreUiSession(readUiSession());
    webAppReady = true;
  }

  async function initializeWebApp() {
    await loadAccountStatus();
    if (accountStatus.signed_in) await prepareAuthenticatedWebApp();
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
    libraries = sortedLibraries(catalogue.libraries);
    librarySyncStatuses = catalogue.sync_statuses;
    libraryTransfers = transfers;
  }

  async function loadAccountSessions() {
    if (!accountStatus.signed_in) {
      accountSessions = [];
      return;
    }
    accountSessions = await commands.getAccountSessions();
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
      transferPreferences = { allow_background_on_metered: false, auto_download_limit_mb: null };
    }
  }

  async function saveTransferPreference(patch: TransferPreferencePatch) {
    const previous = transferPreferences;
    settingsError = "";
    transferPreferences = { ...transferPreferences, [patch.preference]: patch.value };
    try {
      transferPreferences = await commands.updateTransferPreference(patch);
    } catch (reason) {
      transferPreferences = previous;
      settingsError = `Could not save transfer setting: ${errorMessage(reason)}`;
    }
  }

  async function finishAccountAuthentication(status: AccountStatus) {
    accountStatus = status;
    accountProgress = "Fetching libraries…";
    if (isWebApp) await prepareAuthenticatedWebApp();
    else {
      await loadServerTransferState();
      clearThumbnailCache();
      await loadLibraries();
    }
    accountProgress = "";
    if (isWebApp) navigate(libraryRoute("home"), true);
  }

  async function loginToServer(email: string, password: string): Promise<boolean> {
    if (accountBusy) return false;
    accountBusy = true;
    accountError = "";
    accountProgress = "Signing in…";
    try {
      await finishAccountAuthentication(await commands.login(email, password));
      return true;
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
      return false;
    } finally {
      accountBusy = false;
    }
  }

  async function runAccountRequest(progress: string, success: string, request: () => Promise<unknown>): Promise<boolean> {
    if (accountBusy) return false;
    accountBusy = true;
    accountError = "";
    accountProgress = progress;
    try {
      await request();
      accountProgress = success;
      return true;
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
      return false;
    } finally {
      accountBusy = false;
    }
  }

  const registerAccount = (email: string, password: string) => runAccountRequest("Creating account…", "Check your email for the verification token.", () => commands.requestPublicRegistration(email, password));
  const resendAccountVerification = (email: string) => runAccountRequest("Sending verification token…", "If the registration is pending, a new token has been sent.", () => commands.resendVerification(email));
  const requestAccountPasswordReset = (email: string) => runAccountRequest("Requesting password reset…", "If the account exists, a reset token has been sent.", () => commands.requestPasswordReset(email));
  const resetAccountPassword = (token: string, password: string) => runAccountRequest("Updating password…", "Password updated. Sign in with the new password.", () => commands.resetPassword(token, password));

  async function verifyAccountEmail(token: string): Promise<boolean> {
    if (accountBusy) return false;
    accountBusy = true;
    accountError = "";
    accountProgress = "Verifying email…";
    try {
      await finishAccountAuthentication(await commands.verifyEmail(token));
      return true;
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
      return false;
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
      accountSessions = [];
      if (isWebApp) {
        webAppReady = false;
        readerBook = null;
        libraries = [];
        activeLibraryId = "";
        loadingLibrary = true;
        libraryError = "";
        resetLibraryData();
        navigate("/login", true);
      } else {
        await loadLibraries();
      }
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      accountBusy = false;
    }
  }

  async function revokeAccountSession(sessionId: string) {
    if (accountBusy) return;
    const selected = accountSessions.find((session) => session.session_id === sessionId);
    const prompt = selected?.current ? "Sign out this session?" : "Revoke this session? The device will need to sign in again.";
    if (!window.confirm(prompt)) return;
    accountBusy = true;
    accountError = "";
    accountProgress = selected?.current ? "Signing out this session…" : "Revoking session…";
    try {
      accountStatus = await commands.revokeAccountSession(sessionId);
      accountProgress = "";
      if (accountStatus.signed_in) {
        await loadAccountSessions();
      } else {
        accountSessions = [];
        librarySyncStatuses = [];
        libraryTransfers = [];
        if (isWebApp) {
          webAppReady = false;
          readerBook = null;
          libraries = [];
          activeLibraryId = "";
          resetLibraryData();
          navigate("/login", true);
        } else {
          await loadLibraries();
        }
      }
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      accountBusy = false;
    }
  }

  async function revokeOtherAccountSessions() {
    if (accountBusy || accountSessions.length <= 1 || !window.confirm("Sign out every other session? Those devices will need to sign in again.")) return;
    accountBusy = true;
    accountError = "";
    accountProgress = "Signing out other sessions…";
    try {
      const revoked = await commands.revokeOtherAccountSessions();
      await loadAccountSessions();
      accountProgress = revoked === 1 ? "Signed out 1 other session." : `Signed out ${revoked} other sessions.`;
    } catch (reason) {
      accountError = errorMessage(reason);
      accountProgress = "";
    } finally {
      accountBusy = false;
    }
  }

  async function createLibrary(name: string) {
    if (creatingLibrary) return;

    creatingLibrary = true;
    libraryError = "";
    try {
      const library = await measureInvoke("create_library", () => commands.createLibrary(name));
      libraries = sortedLibraries([...libraries.filter((entry) => entry.library_id !== library.library_id), library]);
      showCreateLibrary = false;
      await selectLibrary(library.library_id);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      creatingLibrary = false;
    }
  }

  async function openLibraryFolders() {
    showLibrarySwitcher = false;
    libraryError = "";
    const selected = await openDialog({ title: "Open libraries", directory: true, multiple: true });
    const paths = typeof selected === "string" ? [selected] : selected ?? [];
    if (!paths.length) return;
    const opened: LibraryEntry[] = [];
    const failures: string[] = [];
    for (const path of paths) {
      try {
        opened.push(await commands.openLibraryFolder(path));
      } catch (reason) {
        failures.push(`${path}: ${errorMessage(reason)}`);
      }
    }
    if (opened.length) {
      libraries = sortedLibraries(await commands.getLibraries());
      await selectLibrary(opened.at(-1)!.library_id);
    }
    if (failures.length) libraryError = `Could not open some libraries: ${failures.join("; ")}`;
  }

  async function openFolder(folder: LibraryDirectory) {
    await loadDirectory(folder.id, [...folderStack, { id: folder.id, name: folder.name }]);
    navigate(libraryRoute("library", activeLibraryId, folder.id));
  }

  async function goToFolder(level: number) {
    const nextStack = folderStack.slice(0, level);
    const dirId = nextStack.at(-1)?.id ?? "__root__";
    await loadDirectory(dirId, nextStack);
    navigate(libraryRoute("library", activeLibraryId, dirId));
  }

  async function createFolder(name: string) {
    const parentId = folderStack.at(-1)?.id ?? "__root__";
    const created = await measureInvoke("create_directory", () => commands.createDirectory(activeLibraryId, parentId, name));
    const dirs = directoryContent.directories.filter((folder) => folder.id !== created.id);
    directoryContent = {
      ...directoryContent,
      directories: [...dirs, created].sort((left, right) => naturalCompare(left.name, right.name)),
    };
  }

  async function selectAuthor(authorId: string, authorName: string) {
    const request = ++authorRequest;
    const generation = libraryGeneration;
    authorBooksLoading = true;
    libraryError = "";
    try {
      const view = await measureInvoke("author_books", () => commands.getLibraryAuthors(activeLibraryId, authorId));
      if (request !== authorRequest || generation !== libraryGeneration) return;
      const books = view.books ?? [];
      await commitAndMeasureRender("author_books", () => {
        selectedAuthorId = authorId;
        selectedAuthorName = authorName;
        authorBooks = books;
      }, books.length);
      mergeBookDownloadStates(books);
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
      searchCompletedQuery = "";
      searchPending = false;
      return;
    }
    searchPending = true;
    searchTimer = setTimeout(() => void runSearch(query), SEARCH_DEBOUNCE_MS);
  }

  async function runSearch(query: string) {
    try {
      const results = await measureInvoke("search", () => commands.searchLibrary(activeLibraryId, query, BROWSING_CONTENT_CONTRACT.search_result_limit));
      if (query === searchQuery.trim()) {
        await commitAndMeasureRender("search", () => (searchResults = results), results.length);
        searchCompletedQuery = query;
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

  function loadLibraryViewPreferences() {
    try {
      const saved = localStorage.getItem(BROWSING_UI_PREFERENCES_KEY);
      const parsed = saved ? JSON.parse(saved) : {};
      if (parsed?.library_view_mode === "cover" || parsed?.library_view_mode === "detailed") {
        libraryViewMode = parsed.library_view_mode;
      } else {
        libraryViewMode = DEFAULT_LIBRARY_VIEW_MODE;
      }
    } catch {
      libraryViewMode = DEFAULT_LIBRARY_VIEW_MODE;
    }
  }

  function saveLibraryViewPreference(mode: LibraryViewMode) {
    libraryViewMode = mode;
    try {
      localStorage.setItem(BROWSING_UI_PREFERENCES_KEY, JSON.stringify({ library_view_mode: mode }));
    } catch {
      // Ignore persistence failures for optional local preference state.
    }
  }

  function saveBrowsingPreference(patch: BrowsingPreferencePatch) {
    const previous = browsingPreferences;
    settingsError = "";
    const preferences = { ...browsingPreferences, [patch.preference]: patch.value } as BrowsingPreferences;
    browsingPreferences = preferences;
    const version = ++browsingPreferenceVersion;
    browsingPreferenceSaveQueue = browsingPreferenceSaveQueue.then(async () => {
      try {
        const saved = await commands.updateBrowsingPreference(patch);
        if (version === browsingPreferenceVersion) browsingPreferences = saved;
      } catch (reason) {
        if (version === browsingPreferenceVersion) browsingPreferences = previous;
        settingsError = `Could not save browsing setting: ${errorMessage(reason)}`;
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

  async function openLibraryBook(book: BookCardData) {
    openingBookId = book.book_id;
    libraryError = "";
    try {
      if ((bookDownloadStates[book.book_id] ?? (book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded")) !== "Downloaded") {
        pendingOpenBook = book;
        await measureInvoke("download_book", () => downloadBook(book));
        if (bookDownloadStates[book.book_id] !== "Downloaded") {
          return;
        }
      }
      pendingOpenBook = null;
      const openedBook = await openDownloadedBook(book.book_id, book.title || "Untitled", book.author);
      readerBook = openedBook;
      navigateToReader(book.book_id, openedBook);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      openingBookId = "";
    }
  }

  async function openDownloadedBook(bookId: string, title?: string, author?: string): Promise<ReaderBook> {
      const libraryId = activeLibraryId;
      if (!libraryId) throw new Error("No library is selected");
      const [rawBytes, fileName, initialCfi] = await Promise.all([
        measureInvoke("open_book", () => commands.openBook(libraryId, bookId) as unknown as Promise<BookBytes>),
        commands.getBookFileName(libraryId, bookId),
        measureInvoke("reading_position", () => commands.getBookReadingPosition(libraryId, bookId)),
      ]);
      const buffer = rawBytes instanceof ArrayBuffer
        ? rawBytes
        : rawBytes instanceof Uint8Array
          ? rawBytes.slice().buffer
          : Uint8Array.from(rawBytes).buffer;
      const resolvedTitle = title || fileName.replace(/\.[^.]+$/, "") || "Untitled";
      const extension = fileName.split(".").at(-1)?.toLocaleLowerCase();
      const mediaType = extension === "epub" ? "application/epub+zip"
        : extension === "fb2" ? "application/x-fictionbook+xml"
        : extension === "cbz" ? "application/vnd.comicbook+zip"
        : extension === "webpub" ? "application/webpub+zip"
        : extension === "html" || extension === "htm" ? "text/html"
        : extension === "xhtml" ? "application/xhtml+xml"
        : extension === "mobi" || extension === "azw" || extension === "azw3" ? "application/x-mobipocket-ebook"
        : "application/octet-stream";
      const file = new File([buffer], fileName, { type: mediaType });
      return {
        key: `library:${libraryId}:${bookId}`,
        bookId,
        file,
        title: resolvedTitle,
        author,
        initialCfi,
        saveCfi: async (cfi, progress) => { await measureInvoke("update_reading_position", () => commands.updateReadingPosition(libraryId, bookId, cfi, progress)); },
        loadAnnotations: () => commands.getAnnotations(libraryId, bookId),
        upsertAnnotation: async (annotation) => { await commands.upsertAnnotation(libraryId, annotation); },
        deleteAnnotation: async (annotationId, modifiedAt) => { await commands.deleteAnnotation(libraryId, annotationId, modifiedAt); },
        loadBookmarks: () => commands.getBookmarks(libraryId, bookId),
        createBookmark: async (bookmark) => { await commands.createBookmark(libraryId, bookmark); },
        deleteBookmark: async (bookmarkId, deletedAt) => { await commands.deleteBookmark(libraryId, bookmarkId, deletedAt); },
      };
  }

  async function restoreReaderRoute(bookId: string) {
    if (readerBook?.bookId === bookId || restoringReaderId === bookId) return;
    const cached = readerRouteCache.get(bookId);
    if (cached) {
      readerBook = cached;
      return;
    }
    restoringReaderId = bookId;
    try {
      const knownBook = [...recentlyAdded, ...recentlyRead, ...directoryContent.books, ...authorBooks, ...searchResults]
        .find((book) => book.book_id === bookId);
      const openedBook = await openDownloadedBook(bookId, knownBook?.title, knownBook?.author);
      readerRouteCache.set(bookId, openedBook);
      readerBook = openedBook;
    } catch (reason) {
      libraryError = errorMessage(reason);
      navigate(libraryRoute("home"), true);
    } finally {
      restoringReaderId = "";
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
      const openedBook: ReaderBook = {
        key: storageKey,
        file,
        title: file.name.replace(/\.epub$/i, ""),
        initialCfi: localStorage.getItem(storageKey),
        saveCfi: (cfi) => localStorage.setItem(storageKey, cfi),
      };
      readerBook = openedBook;
      navigateToReader(storageKey, openedBook);
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
      picked = await pickEpub(true);
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
      await measureInvoke("import_book", () => commands.importBook({ library_id: activeLibraryId, directory_id: parentId }, sourcePath, picked.name));
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
    let unlistenTransferStatus: (() => void) | undefined;
    let transferRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    loadLibraryViewPreferences();
    const unobserveNetwork = observeNetworkStatus((status) => {
      networkStatus = status;
      if (!isWebApp || accountStatus.signed_in) {
        void commands.updateNetworkStatus(status).then((reported) => (networkStatus = reported)).catch(() => undefined);
      }
    });
    if (isWebApp) {
      void initializeWebApp();
    } else {
      void (async () => {
        await loadLibraries();
        await restoreUiSession(readUiSession());
        await loadAccountStatus();
        await loadBrowsingPreferences();
        await loadReaderPreferences();
        await loadTransferPreferences();
      })();
    }
    void listen<DownloadStatusEvent>("library:download-status", ({ payload }) => {
      if (payload.library_id !== activeLibraryId) return;
      const change = payload.event.DownloadStatusChanged;
      bookDownloadStates = { ...bookDownloadStates, [change.book_id]: change.state };
      if (pendingOpenBook?.book_id === change.book_id && change.state === "Downloaded" && openingBookId !== change.book_id) {
        const book = pendingOpenBook;
        pendingOpenBook = null;
        queueMicrotask(() => void openLibraryBook(book));
      } else if (pendingOpenBook?.book_id === change.book_id && change.state === "NotDownloaded") {
        libraryError = "Could not download the book.";
      }
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
    void listen("library:transfer-status", () => {
      if (activeDestination !== "account" || !accountStatus.signed_in) return;
      if (transferRefreshTimer) clearTimeout(transferRefreshTimer);
      transferRefreshTimer = setTimeout(() => void loadServerTransferState().catch((reason) => (accountError = errorMessage(reason))), 120);
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenTransferStatus = unlisten;
    }).catch(() => undefined);
    const handleShortcut = (event: KeyboardEvent) => {
      if (isWebApp || readerBook || !(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
      const destination = ({ "1": "home", "2": "library", "3": "authors", "4": "search", "5": "store", "6": "account", "7": "settings" } as const)[event.key];
      if (destination) {
        event.preventDefault();
        setDestination(destination);
      } else if (event.key.toLocaleLowerCase() === "f") {
        event.preventDefault();
        setDestination("search");
      }
    };
    document.addEventListener("keydown", handleShortcut);
    return () => {
      disposed = true;
      unlistenDownloadStatus?.();
      unlistenFolderDownloadStatus?.();
      unlistenTransferStatus?.();
      if (transferRefreshTimer) clearTimeout(transferRefreshTimer);
      document.removeEventListener("keydown", handleShortcut);
      unobserveNetwork();
      if (searchTimer) clearTimeout(searchTimer);
      clearThumbnailCache();
    };
  });
</script>

<svelte:head><title>{isWebApp && !accountStatus.signed_in ? `Account · ${APP_NAME}` : readerBook ? `${readerBook.title} · Reader` : `${destinationTitle} · ${APP_NAME}`}</title></svelte:head>

{#if isWebApp && (!accountResolved || (accountStatus.signed_in && !webAppReady))}
  <main class="auth-loading" aria-live="polite">
    <span class="loader"></span>
    <strong>{accountResolved ? "Opening your library" : "Checking your session"}</strong>
  </main>
{:else if isWebApp && !accountStatus.signed_in}
  <LoginScreen
    busy={accountBusy}
    progress={accountProgress}
    error={accountError}
    onlogin={loginToServer}
    onregister={registerAccount}
    onresendverification={resendAccountVerification}
    onverifyemail={verifyAccountEmail}
    onrequestpasswordreset={requestAccountPasswordReset}
    onresetpassword={resetAccountPassword}
  />
{:else if readerBook}
  {#key readerBook.key}
    <Reader
      book={readerBook}
      preferences={readerPreferences}
      applicationTheme={browsingPreferences.theme}
      onpreferenceschange={saveReaderPreference}
      onclose={closeReader}
    />
  {/key}
{:else}
  <div class="adaptive-shell" style={browsingLayoutStyle}>
    <AdaptiveNavigation
      active={activeDestination}
      libraryName={activeLibrary?.library_name ?? "Libraries"}
      onnavigate={setDestination}
      onlibraries={openLibrarySwitcher}
      onaccount={() => setDestination("account")}
      onsettings={() => setDestination("settings")}
    />

    <div class="app-column">
      <AppTopbar
        pageTitle={destinationTitle}
        libraryName={activeLibrary?.library_name ?? "Libraries"}
        folders={folderStack}
        showBreadcrumb={activeDestination === "library"}
        showLibraryActions={activeDestination === "library"}
        {addingBook}
        accountSignedIn={accountStatus.signed_in}
        onlibraries={openLibrarySwitcher}
        onaccount={() => setDestination("account")}
        onsettings={() => setDestination("settings")}
        onaddbook={addBookToCurrentFolder}
        onnewfolder={() => (showCreateFolder = true)}
        onfolder={goToFolder}
      />

      <main class:library-view={activeDestination === "library"} class:authors-view={activeDestination === "authors"} class:store-view={activeDestination === "store"} class="content">
        {#if activeDestination === "account"}
          <ServerAccountDialog
            account={accountStatus}
            {accountBusy}
            {accountProgress}
            {accountError}
            {librarySyncStatuses}
            {libraryTransfers}
            {accountSessions}
            onlogin={loginToServer}
            onlogout={logoutFromServer}
            onrevokesession={revokeAccountSession}
            onrevokeothersessions={revokeOtherAccountSessions}
            onregister={registerAccount}
            onresendverification={resendAccountVerification}
            onverifyemail={verifyAccountEmail}
            onrequestpasswordreset={requestAccountPasswordReset}
            onresetpassword={resetAccountPassword}
          />
        {:else if activeDestination === "settings"}
          <BrowsingSettingsDialog
            preferences={browsingPreferences}
            {networkStatus}
            {transferPreferences}
            {libraryViewMode}
            error={settingsError}
            onchange={saveBrowsingPreference}
            ontransferpreference={saveTransferPreference}
            onlibraryviewmode={saveLibraryViewPreference}
          />
        {:else if activeDestination === "store" && !activeLibraryId}
          <EmptyPage
            title={BROWSING_STATES_CONTRACT.no_libraries_title}
            description={BROWSING_STATES_CONTRACT.no_libraries_description}
            primaryLabel={BROWSING_STATES_CONTRACT.create_library_label}
            onprimary={() => setDestination("library")}
          />
        {:else if activeDestination === "store"}
          <Store {activeLibraryId} libraryName={activeLibrary?.library_name ?? "library"} onimported={storeBookImported} />
        {:else if loadingLibrary}
          <section class="page-state" aria-live="polite">
            <span class="loader"></span>
            <strong>{BROWSING_STATES_CONTRACT.loading_library_label}</strong>
          </section>
        {:else if !libraries.length}
          <EmptyPage title={BROWSING_STATES_CONTRACT.no_libraries_title} description={BROWSING_STATES_CONTRACT.no_libraries_description} primaryLabel={BROWSING_STATES_CONTRACT.create_library_label} onprimary={() => { showCreateLibrary = true; }} />
        {:else if activeDestination === "home"}
          {#if !visibleRecentlyInProgress.length && !visibleRecentlyFinished.length && !visibleRecentlyAdded.length}
            <EmptyPage primaryLabel="Add book" onprimary={addBookToCurrentFolder} />
          {:else}
          {#if visibleRecentlyInProgress.length}
            <BookShelf
              libraryId={activeLibraryId}
              id="recently-read-carousel"
              title="Recently read"
              books={visibleRecentlyInProgress}
              emptyMessage="No books in progress yet."
              first
              {openingBookId}
              downloadStates={bookDownloadStates}
              onopen={openLibraryBook}
              ondownload={downloadBook}
              onremove={removeLocalBookCopy}
            />
          {/if}

          {#if visibleRecentlyFinished.length}
            <BookShelf
              libraryId={activeLibraryId}
              id="finished-carousel"
              title="Finished"
              books={visibleRecentlyFinished}
              emptyMessage="No finished books yet."
              first={!visibleRecentlyInProgress.length}
              {openingBookId}
              downloadStates={bookDownloadStates}
              onopen={openLibraryBook}
              ondownload={downloadBook}
              onremove={removeLocalBookCopy}
            />
          {/if}

          {#if visibleRecentlyAdded.length}<BookShelf
            libraryId={activeLibraryId}
            id="recently-added-carousel"
            title="Recently added"
            books={visibleRecentlyAdded}
            emptyMessage="No books have been added yet."
            first={!visibleRecentlyInProgress.length && !visibleRecentlyFinished.length}
            maxItems={BROWSING_CONTENT_CONTRACT.home_book_limit}
            {openingBookId}
            downloadStates={bookDownloadStates}
            onopen={openLibraryBook}
            ondownload={downloadBook}
            onremove={removeLocalBookCopy}
          />{/if}
          {/if}
        {:else if activeDestination === "library"}
          <section class="content-section library-section">
              {#if directoryContent.directories.length}
                <div class="folder-grid">
                  {#each directoryContent.directories.filter((folder) => folder.id !== "__root__") as folder}
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
                libraryId={activeLibraryId}
                books={visibleDirectoryBooks}
                detailed={libraryViewMode === "detailed"}
                emptyMessage=""
                {openingBookId}
                downloadStates={bookDownloadStates}
                onopen={openLibraryBook}
                ondownload={downloadBook}
                onremove={removeLocalBookCopy}
              />
              {#if !directoryContent.directories.length && !visibleDirectoryBooks.length}
                <EmptyPage
                  title={folderStack.length ? BROWSING_STATES_CONTRACT.empty_folder_title : BROWSING_STATES_CONTRACT.empty_library_title}
                  description={folderStack.length ? BROWSING_STATES_CONTRACT.empty_folder_description : BROWSING_STATES_CONTRACT.empty_library_description}
                  primaryLabel="Add book"
                  secondaryLabel="New folder"
                  onprimary={addBookToCurrentFolder}
                  onsecondary={() => { showCreateFolder = true; }}
                />
              {/if}
          </section>
        {:else if activeDestination === "authors"}
          <section class="authors-layout" aria-label="Authors">
            <div class:has-selection={selectedAuthorId} class="author-list-panel">
              <label class="author-filter">
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
                <input id="author-filter" type="search" bind:value={authorFilter} onkeydown={handleAuthorFilterKeydown} autocomplete="off" aria-label="Filter authors" />
              </label>
              {#if filteredAuthors.length}
                <div class="author-list-scroll">
                  <div class="author-list">
                    {#each visibleAuthors as author, index}
                      <button
                        class:active={selectedAuthorId === author.id}
                        type="button"
                        data-author-index={index}
                        tabindex={index === focusedAuthorIndex ? 0 : -1}
                        onfocus={() => (focusedAuthorIndex = index)}
                        onkeydown={(event) => handleAuthorKeydown(event, index)}
                        onclick={() => { focusedAuthorIndex = index; void selectAuthor(author.id, author.name); }}
                      >
                        <span class="author-avatar">{author.name.slice(0, 1).toUpperCase() || "?"}</span>
                        <span><strong>{author.name || "Unknown author"}</strong><small>{author.book_count} {author.book_count === 1 ? "book" : "books"}</small></span>
                        <i>›</i>
                      </button>
                    {/each}
                  </div>
                </div>
              {:else}
                {#if authorFilter.trim()}
                  <div class="inline-empty">No matching authors.</div>
                {:else}
                  <EmptyPage title="No authors" primaryLabel="Add book" onprimary={addBookToCurrentFolder} />
                {/if}
              {/if}
            </div>

            <div class="author-books-panel">
              {#if selectedAuthorId}
                <div class="section-heading author-heading">
                  <button class="author-back" type="button" onclick={clearSelectedAuthor}>← <span>Authors</span></button>
                <div><h2>{selectedAuthorName}</h2><p>{visibleAuthorBooks.length} {visibleAuthorBooks.length === 1 ? "book" : "books"}</p></div>
                </div>
                <BookGrid
                  libraryId={activeLibraryId}
                  books={visibleAuthorBooks}
                  detailed={libraryViewMode === "detailed"}
                  emptyMessage="No books found for this author."
                  {openingBookId}
                  downloadStates={bookDownloadStates}
                  onopen={openLibraryBook}
                  ondownload={downloadBook}
                  onremove={removeLocalBookCopy}
                />
                {#key selectedAuthorId}
                  <AuthorCatalogResults authorId={selectedAuthorId} authorName={selectedAuthorName} {activeLibraryId} onimported={storeBookImported} />
                {/key}
              {/if}
            </div>
          </section>
        {:else}
          <section class="search-page">
            <label class="search-field">
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
              <input id="library-search" type="search" bind:value={searchQuery} oninput={scheduleSearch} placeholder="Title" autocomplete="off" />
            </label>
            {#if searchCompletedQuery}
              <BookGrid
                libraryId={activeLibraryId}
                books={visibleSearchResults}
                detailed={libraryViewMode === "detailed"}
                emptyMessage={BROWSING_STATES_CONTRACT.no_search_results_label}
                {openingBookId}
                downloadStates={bookDownloadStates}
                onopen={openLibraryBook}
                ondownload={downloadBook}
                onremove={removeLocalBookCopy}
              />
            {/if}
          </section>
        {/if}

        {#if libraryError}
          <div class="error-banner" role="alert"><span>Library unavailable: {libraryError}</span><button type="button" onclick={() => void retryActiveSection()}>{BROWSING_STATES_CONTRACT.retry_label}</button></div>
        {/if}
      </main>
    </div>

    {#if showLibrarySwitcher}
      <LibrarySwitcher
        {libraries}
        {activeLibraryId}
        onclose={() => (showLibrarySwitcher = false)}
        onselect={selectLibrary}
        oncreate={() => { showLibrarySwitcher = false; showCreateLibrary = true; }}
        onopen={openLibraryFolders}
      />
    {/if}

    {#if showCreateFolder}
      <CreateFolderDialog onclose={() => (showCreateFolder = false)} oncreate={createFolder} />
    {/if}

    {#if showCreateLibrary}
      <CreateLibraryDialog busy={creatingLibrary} onclose={() => (showCreateLibrary = false)} oncreate={createLibrary} />
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
  :global(:where(button, a, input, select, textarea, [tabindex]):focus-visible) { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  button, input { font: inherit; }
  button, label { -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }

  .auth-loading { display: grid; min-height: 100dvh; place-content: center; justify-items: center; gap: .8rem; color: var(--color-subtle-text); background: var(--color-surface-bg); font-size: .8rem; }

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

  .section-heading h2 {
    margin: 0;
    font-family: var(--font-heading-family);
    font-weight: 500;
    letter-spacing: -.035em;
  }
  .content-section { margin-top: 2rem; }
  .section-heading { display: flex; align-items: end; justify-content: space-between; gap: 1rem; margin-bottom: .9rem; }
  .section-heading h2 { font-size: clamp(1.55rem, 7vw, 2.15rem); }

  .page-state {
    display: grid;
    place-items: center;
    min-height: 17rem;
    padding: 2rem;
    text-align: center;
    color: var(--color-subtle-text);
  }
  .page-state { gap: .75rem; }
  .loader { width: 1.4rem; height: 1.4rem; border: .18rem solid var(--color-border-color); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .inline-empty { display: grid; place-items: center; min-height: 8rem; color: var(--color-subtle-text); font-size: .84rem; text-align: center; }

  .library-section { margin-top: 0; }
  .folder-grid { display: flex; flex-wrap: wrap; align-items: flex-start; gap: .45rem; margin-bottom: 1.25rem; }

  .content.authors-view { display: flex; flex-direction: column; overflow: hidden; }
  .authors-layout { display: grid; flex: 1; width: min(1200px, 100%); min-height: 0; margin: 0 auto; gap: 2rem; overflow: hidden; }
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

  .search-field { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: .7rem; min-height: 3.6rem; padding: 0 1rem; border: 1px solid var(--color-border-color); border-radius: 1rem; background: color-mix(in srgb, var(--color-white-surface) 70%, transparent); box-shadow: 0 .7rem 2rem color-mix(in srgb, var(--color-box-shadow) 20%, transparent); }
  .search-field svg { width: 1.25rem; color: var(--color-accent); }
  .search-field input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font-size: 1rem; }

  .error-banner { display: flex; align-items: center; justify-content: space-between; gap: 1rem; margin: 1rem 0; padding: .75rem 1rem; border-radius: .75rem; color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .75rem; overflow-wrap: anywhere; }
  .error-banner button { flex: none; min-height: 2.2rem; padding: .4rem .7rem; border: 1px solid currentColor; border-radius: var(--radius-sm); color: inherit; background: transparent; font-weight: 750; cursor: pointer; }

  @media (min-width: 45rem) {
    .adaptive-shell { --page-gutter: var(--layout-content-padding); display: grid; grid-template-columns: var(--layout-sidebar-width) minmax(0, 1fr); }
    .content { padding-top: var(--layout-content-padding); padding-right: max(var(--page-gutter), env(safe-area-inset-right)); padding-bottom: 3rem; padding-left: max(var(--page-gutter), env(safe-area-inset-left)); }
    .content.authors-view { padding-bottom: 1.5rem; }
    .authors-layout { grid-template-columns: minmax(0, 1fr) minmax(0, 2fr); align-items: stretch; }
    .author-list-panel.has-selection { display: flex; }
    .author-books-panel { display: block; }
    .author-back { display: none; }
    .author-list-panel { position: static; max-height: none; overflow: hidden; }
  }

  @media (min-width: 88rem) {
    .adaptive-shell { --page-gutter: 2rem; }
  }

  @media (prefers-reduced-motion: reduce) {
    .loader { animation: none; }
  }

  /* Compact library UI, aligned with the Floem desktop application. */
  .adaptive-shell { background: var(--color-app-bg); }
  .content { max-width: none; padding-top: 1.1rem; }
  .content-section { margin-top: 1.8rem; }
  .section-heading { margin-bottom: .8rem; }
  .section-heading h2 { font-size: 1.35rem; letter-spacing: -.02em; }
  .inline-empty { min-height: 5rem; justify-items: start; text-align: left; }
  .search-field { border-radius: .65rem; box-shadow: none; }
  .author-list button { border-radius: .55rem; }
  .author-list button.active { background: var(--color-active-highlight); }

</style>
