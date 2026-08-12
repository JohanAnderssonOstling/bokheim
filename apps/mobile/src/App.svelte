<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
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
  import { applyApplicationAppearance, observeSystemAppearance, resolveAppearance } from "./lib/application-appearance";
  import { commands } from "./lib/commands";
  import { toBackendDirectoryId } from "./lib/directory-id";
  import { pickEpub, type PickedEpub } from "./lib/epub-picker";
  import { NAV_ITEMS, type Destination } from "./lib/navigation";
  import { observeNetworkStatus, UNKNOWN_NETWORK_STATUS } from "./lib/network-status";
  import { commitAndMeasureRender, measureInvoke } from "./lib/performance";
  import { clearThumbnailCache } from "./lib/thumbnail-cache";
  import { readUiSession, writeUiSession, type AppUiSession } from "./lib/ui-session";
  import {
    ACCOUNT_CONTRACT,
    BROWSING_CONTENT_CONTRACT,
    BROWSING_LAYOUT_CONTRACT,
    BROWSING_STATES_CONTRACT,
    DEFAULT_READER_PREFERENCES,
    DEFAULT_BROWSING_PREFERENCES,
    type AccountStatus,
    type AccountStorageUsage,
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
    type LocalLibraryStorageUsage,
    type ServerLibraryStorageUsage,
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
  type DownloadStatusEvent = {
    library_id: string;
    event: { DownloadStatusChanged: { content_hash: string; state: DownloadState } };
  };
  type FolderDownloadStatusEvent = {
    library_id: string;
    event: { FolderDownloadStatusChanged: { dir_id: string; downloaded_books: number; total_books: number } };
  };

  const EMPTY_DIRECTORY: DirectoryContent = { books: [], children: [] };
  const SEARCH_DEBOUNCE_MS = BROWSING_CONTENT_CONTRACT.search_debounce_ms;
  const naturalCollator = new Intl.Collator(undefined, { sensitivity: "base", numeric: true });
  const WEB_PREFERENCES_KEY = `${APP_NAME_LOWER}-reader-preferences`;

  function currentSessionFromRoute(): AppUiSession | null {
    const libraryId = (page.url.searchParams.get("library") || page.params.libraryId || undefined)?.trim() || undefined;
    const routeId = page.route.id;
    if (routeId === "/reader/[bookId]" && page.params.bookId && page.params.bookId.trim()) {
      return {
        kind: "reader",
        content_hash: page.params.bookId,
        library_id: libraryId,
      };
    }
    if (routeId === "/library/[libraryId]/folder/[folderId]") {
      const folderId = page.params.folderId?.trim() || "__root__";
      return {
        kind: "browse",
        destination: "folders",
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
    if (routeId === "/library/[libraryId]/subjects/[subjectId]") {
      const subjectId = page.params.subjectId?.trim();
      return {
        kind: "browse",
        destination: "subjects",
        library_id: page.params.libraryId || libraryId,
        subject_id: subjectId === "__root__" ? undefined : subjectId,
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

  function naturalCompare(left: string, right: string) {
    return naturalCollator.compare(left, right);
  }

  function subjectBreadcrumb(subjectId: string): FolderLevel[] {
    const names = subjectId.split(" / ").map((name) => name.trim()).filter(Boolean);
    return names.map((name, index) => ({ id: names.slice(0, index + 1).join(" / "), name }));
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
  let subjectContent = $state<DirectoryContent>(EMPTY_DIRECTORY);
  let subjectStack = $state<FolderLevel[]>([]);
  let subjectsLoading = $state(false);
  let subjectsLoadedFor = $state("");
  let subjectRequest = 0;
  let authors = $state<AuthorSummary[]>([]);
  let authorFilter = $state("");
  let focusedAuthorIndex = $state(0);
  let selectedAuthorId = $state("");
  let selectedAuthorName = $state("");
  let authorBooks = $state<BookCardData[]>([]);
  let searchQuery = $state("");
  let titleSearchResults = $state<BookCardData[]>([]);
  let authorSearchResults = $state<BookCardData[]>([]);
  let searchCompletedQuery = $state("");
  let searchCompletedScope = $state("");
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
  let accountStatus = $state<AccountStatus>({ signed_in: false, server_url: null, email: null });
  let accountStorageUsage = $state<AccountStorageUsage | null>(null);
  let accountBusy = $state(false);
  let accountProgress = $state("");
  let accountError = $state("");
  let accountStatusError = $state("");
  let settingsError = $state("");
  let accountResolved = $state(false);
  let webAppReady = $state(!isWebApp);
  let librarySyncStatuses = $state<LibrarySyncStatus[]>([]);
  let libraryTransfers = $state<LibraryTransfers[]>([]);
  let localLibraryStorageUsage = $state<LocalLibraryStorageUsage[]>([]);
  /** Null until a server that reports storage per library answers. */
  let serverLibraryStorageUsage = $state<ServerLibraryStorageUsage[] | null>(null);
  let networkStatus = $state<NetworkStatus>({ ...UNKNOWN_NETWORK_STATUS });
  let transferPreferences = $state<TransferPreferences>({ allow_background_on_metered: false, auto_download_limit_mb: null });
  let browsingPreferenceSaveQueue = Promise.resolve();
  let browsingPreferenceVersion = 0;
  let preferenceSaveQueue = Promise.resolve();
  let preferenceVersion = 0;
  let restoringReaderId = "";
  let readerReturnPath = "";
  let routesReady = $state(false);
  const readerRouteCache = new Map<string, ReaderBook>();

  const destinationTitle = $derived(activeDestination === "account" ? "Account" : activeDestination === "settings" ? "Settings" : NAV_ITEMS.find((item) => item.id === activeDestination)?.label ?? APP_NAME);
  const activeLibrary = $derived(libraries.find((library) => library.library_id === activeLibraryId));
  const visibleDirectoryBooks = $derived(directoryContent.books);
  const visibleSubjectBooks = $derived(subjectContent.books);
  const visibleAuthorBooks = $derived(authorBooks);
  const visibleTitleSearchResults = $derived(titleSearchResults);
  const visibleAuthorSearchResults = $derived(authorSearchResults);
  const visibleRecentlyInProgress = $derived(recentlyInProgress);
  const visibleRecentlyFinished = $derived(recentlyFinished);
  const visibleRecentlyAdded = $derived(recentlyAdded);
  const filteredAuthors = $derived(authors.filter((author) => author.name.toLocaleLowerCase().includes(authorFilter.trim().toLocaleLowerCase())));
  const visibleAuthors = $derived(filteredAuthors);

  function libraryRoute(destination: Destination = activeDestination, libraryId = activeLibraryId, locationId?: string) {
    if (destination === "account") return "/account";
    if (destination === "settings") return "/settings";
    if (destination === "store") return "/store";
    if (!libraryId) return "/library";
    const library = `/library/${encodeURIComponent(libraryId)}`;
    if (destination === "folders") return locationId && locationId !== "__root__" ? `${library}/folder/${encodeURIComponent(locationId)}` : `${library}/folder/__root__`;
    if (destination === "subjects") return `${library}/subjects/${encodeURIComponent(locationId || "__root__")}`;
    if (destination === "authors") return `${library}/authors`;
    return library;
  }

  async function navigate(path: string, replaceState = false) {
    if (`${page.url.pathname}${page.url.search}` !== path) await goto(path, { replaceState, noScroll: true, keepFocus: true });
  }

  function closeReader() {
    if (page.route.id === "/reader/[bookId]") {
      const destination = readerReturnPath || libraryRoute("home", activeLibraryId);
      readerBook = null;
      navigate(destination, true);
    } else {
      readerBook = null;
    }
  }

  function navigateToReader(bookId: string, book: ReaderBook) {
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

  // Without this, "follow system" would only be honoured at startup.
  $effect(() => observeSystemAppearance(() => applyApplicationAppearance(browsingPreferences)));

  $effect(() => {
    authorFilter;
    focusedAuthorIndex = 0;
  });

  $effect(() => {
    if (!routesReady) return;
    const route = {
      id: page.route.id,
      pathname: page.url.pathname,
      libraryId: page.params.libraryId,
      folderId: page.params.folderId,
      subjectId: page.params.subjectId,
      bookId: page.params.bookId,
      readerLibraryId: page.url.searchParams.get("library") ?? undefined,
    };
    void synchronizeRoute(route);
  });

  $effect(() => {
    if (!routesReady) return;
    const session = buildSessionForPersistence();
    writeUiSession(session);
  });

  $effect(() => {
    if (activeDestination !== "account") return;
    void refreshAccountView();
  });

  function errorMessage(reason: unknown) {
    return reason instanceof Error ? reason.message : String(reason);
  }

  async function synchronizeRoute(route: {
    id: string | null;
    pathname: string;
    libraryId?: string;
    folderId?: string;
    subjectId?: string;
    bookId?: string;
    readerLibraryId?: string;
  }) {
    if (isWebApp && !accountResolved) return;
    if (isWebApp && !accountStatus.signed_in) {
      readerBook = null;
      if (route.id !== "/login") await navigate("/login", true);
      return;
    }
    if (isWebApp && !webAppReady) return;

    if (route.id === "/" || route.id === "/login" || route.id === "/library") {
      await navigate(libraryRoute("home"), true);
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

    const destination: Destination = route.id === "/library/[libraryId]/subjects/[subjectId]"
      ? "subjects"
      : route.id === "/library/[libraryId]/authors"
      ? "authors"
      : route.id === "/library/[libraryId]/folder/[folderId]"
          ? "folders"
          : "home";

    if (route.libraryId && route.libraryId !== activeLibraryId && libraries.some((entry) => entry.library_id === route.libraryId)) {
      await selectLibrary(route.libraryId, false);
    }
    if (!activeLibraryId) return;
    activeDestination = destination;
    libraryError = "";
    if (destination === "folders") {
      const folderId = route.folderId ?? "__root__";
      const stack = folderId === "__root__"
        ? []
        : folderStack.at(-1)?.id === folderId
          ? folderStack
          : (await commands.getLibraryFolderPath(activeLibraryId, toBackendDirectoryId(folderId))).map((folder) => ({ id: folder.id, name: folder.name }));
      await loadDirectory(folderId, stack);
    } else if (destination === "subjects") {
      const subjectId = route.subjectId && route.subjectId !== "__root__" ? route.subjectId : undefined;
      const stack = subjectId
        ? subjectStack.at(-1)?.id === subjectId
          ? subjectStack
          : subjectBreadcrumb(subjectId)
        : [];
      await loadSubjects(subjectId, stack);
    } else {
      await ensureDestination(destination);
    }
  }

  function buildSessionForPersistence(): AppUiSession | null {
    const session = currentSessionFromRoute();
    if (!session) return null;
    if (session.kind === "reader" && session.content_hash.startsWith("foliate-position:")) return null;
    return session;
  }

  function routeFromSession(session: AppUiSession): string {
    if (session.kind === "reader") {
      const library = session.library_id ? `?library=${encodeURIComponent(session.library_id)}` : "";
      return `/reader/${encodeURIComponent(session.content_hash)}${library}`;
    }
    if (session.destination === "account") return "/account";
    if (session.destination === "settings") return "/settings";
    if (session.destination === "store") return "/store";
    if (session.destination === "folders") {
      return libraryRoute("folders", session.library_id, session.folder_id);
    }
    if (session.destination === "authors") {
      return libraryRoute("authors", session.library_id);
    }
    if (session.destination === "subjects") {
      return libraryRoute("subjects", session.library_id, session.subject_id);
    }
    return libraryRoute("home", session.library_id);
  }

  async function restoreUiSession(session: AppUiSession | null) {
    if (!session) return false;
    const current = `${page.url.pathname}${page.url.search}`;
    if (page.route.id !== "/" && page.route.id !== "/library" && page.route.id !== "/login" && page.route.id !== null) return false;
    const route = routeFromSession(session);
    if (route && route !== current) await navigate(route, true);
    return true;
  }

  function setDestination(destination: Destination, updateRoute = true) {
    const enteringAccount = destination === "account" && activeDestination !== "account";
    if (destination !== activeDestination) clearLibrarySearch();
    activeDestination = destination;
    libraryError = "";
    void ensureDestination(destination);
    const locationId = destination === "subjects" ? subjectStack.at(-1)?.id : folderStack.at(-1)?.id;
    if (updateRoute) navigate(libraryRoute(destination, activeLibraryId, locationId));
    if (enteringAccount) void refreshAccountView();
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
    if (activeDestination === "folders") await loadDirectory(folderStack.at(-1)?.id ?? "__root__", folderStack, libraryGeneration, true);
    if (activeDestination === "subjects") await loadSubjects(subjectStack.at(-1)?.id, subjectStack, libraryGeneration, true);
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
    subjectContent = EMPTY_DIRECTORY;
    subjectStack = [];
    subjectsLoadedFor = "";
    subjectsLoading = false;
    subjectRequest += 1;
    authors = [];
    authorFilter = "";
    focusedAuthorIndex = 0;
    selectedAuthorId = "";
    selectedAuthorName = "";
    authorBooks = [];
    folderStack = [];
    searchQuery = "";
    titleSearchResults = [];
    authorSearchResults = [];
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
    for (const book of books) next[book.content_hash] = book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded";
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
    if (bookDownloadStates[book.content_hash] === "Downloaded") return Promise.resolve();
    bookDownloadStates = { ...bookDownloadStates, [book.content_hash]: "Queued" };
    try {
      const state = await commands.downloadBook(activeLibraryId, book.content_hash);
      if (bookDownloadStates[book.content_hash] !== "Downloaded") bookDownloadStates = { ...bookDownloadStates, [book.content_hash]: state };
    } catch (reason) {
      libraryError = errorMessage(reason);
      throw reason;
    }
  }

  async function removeLocalBookCopy(book: BookCardData) {
    if (!window.confirm(`Remove the downloaded copy of “${book.title || "Untitled"}” from this device? You can download it again later.`)) return;
    libraryError = "";
    try {
      const state = await commands.removeLocalBookCopy(activeLibraryId, book.content_hash);
      bookDownloadStates = { ...bookDownloadStates, [book.content_hash]: state };
    } catch (reason) {
      libraryError = errorMessage(reason);
    }
  }

  async function downloadFolder(folder: LibraryDirectory) {
    if (activeFolderDownloads[folder.id]) return;
    activeFolderDownloads = { ...activeFolderDownloads, [folder.id]: true };
    libraryError = "";
    try {
      const status = await commands.downloadFolder(activeLibraryId, toBackendDirectoryId(folder.id));
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
    if (destination === "folders") {
      const dirId = folderStack.at(-1)?.id ?? "__root__";
      if (folderLoadedFor !== `${activeLibraryId}:${dirId}`) await loadDirectory(dirId, folderStack, generation);
    }
    if (destination === "subjects") {
      const subjectId = subjectStack.at(-1)?.id;
      const key = `${activeLibraryId}:${subjectId ?? "__root__"}`;
      if (subjectsLoadedFor !== key) await loadSubjects(subjectId, subjectStack, generation);
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
      const content = await measureInvoke("directory", () => commands.getLibraryFolder(libraryId, toBackendDirectoryId(dirId)));
      if (request !== folderRequest || generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      const sortedContent = {
        ...content,
        children: [...content.children].sort((left, right) => naturalCompare(left.name, right.name)),
        books: [...content.books].sort((left, right) => naturalCompare(left.title, right.title)),
      };
      await commitAndMeasureRender("directory", () => {
        directoryContent = sortedContent;
        folderStack = nextStack;
        folderLoadedFor = key;
      }, content.books.length + content.children.length);
      mergeBookDownloadStates(content.books);
      replaceFolderDownloadStates(content.children);
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

  async function loadSubjects(subjectId: string | undefined, nextStack: FolderLevel[], generation = libraryGeneration, force = false) {
    const libraryId = activeLibraryId;
    const key = `${libraryId}:${subjectId ?? "__root__"}`;
    if (!libraryId || subjectsLoading || (!force && subjectsLoadedFor === key)) return;
    const request = ++subjectRequest;
    subjectsLoading = true;
    libraryError = "";
    try {
      const view = await measureInvoke("subjects", () => commands.getLibrarySubjects(libraryId, subjectId ?? null));
      if (request !== subjectRequest || generation !== libraryGeneration || libraryId !== activeLibraryId) return;
      subjectContent = {
        children: [...view.children].sort((left, right) => naturalCompare(left.name, right.name)),
        books: [...view.books].sort((left, right) => naturalCompare(left.title, right.title)),
      };
      subjectStack = nextStack;
      subjectsLoadedFor = key;
      mergeBookDownloadStates(view.books);
    } catch (reason) {
      if (request === subjectRequest && generation === libraryGeneration) libraryError = errorMessage(reason);
    } finally {
      if (request === subjectRequest && generation === libraryGeneration) subjectsLoading = false;
    }
  }

  async function openSubject(subject: LibraryDirectory) {
    clearLibrarySearch();
    const nextStack = [...subjectStack, { id: subject.id, name: subject.name }];
    await loadSubjects(subject.id, nextStack);
    await navigate(libraryRoute("subjects", activeLibraryId, subject.id));
  }

  async function goToSubject(level: number) {
    clearLibrarySearch();
    const nextStack = level < 0 ? [] : subjectStack.slice(0, level + 1);
    const subjectId = nextStack.at(-1)?.id;
    await loadSubjects(subjectId, nextStack);
    await navigate(libraryRoute("subjects", activeLibraryId, subjectId));
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
    accountResolved = false;
    try {
      const status = await commands.getAccountStatus();
      accountStatus = status;
      accountStatusError = "";
    } catch (reason) {
      accountStatusError = `Could not check your session: ${errorMessage(reason)}`;
    } finally {
      accountResolved = true;
    }
  }

  async function refreshAccountView() {
    await loadAccountStatus();
    if (!accountStatusError && accountStatus.signed_in) {
      await loadServerTransferState().catch((reason) => (accountError = errorMessage(reason)));
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
    routesReady = true;
    webAppReady = true;
  }

  async function initializeWebApp() {
    await loadAccountStatus();
    if (accountStatus.signed_in) await prepareAuthenticatedWebApp();
    else routesReady = true;
  }

  async function loadServerTransferState() {
    if (!accountStatus.signed_in) {
      librarySyncStatuses = [];
      libraryTransfers = [];
      localLibraryStorageUsage = [];
      accountStorageUsage = null;
      serverLibraryStorageUsage = null;
      return;
    }
    const [catalogue, transfers, storageUsage, localStorageUsage, serverStorageUsage] = await Promise.all([
      commands.reconcileLibraryCatalogue(),
      commands.getTransferStatuses(),
      // Older servers do not expose the optional account quota endpoint yet.
      // Keep catalogue and transfer synchronization usable in that case.
      commands.getAccountStorageUsage().catch(() => null),
      commands.getLocalLibraryStorageUsage(),
      // Null both when the server predates the per-library endpoint and when
      // the call fails: either way there is no breakdown to show.
      commands.getServerLibraryStorageUsage().catch(() => null),
    ]);
    libraries = sortedLibraries(catalogue.libraries);
    librarySyncStatuses = catalogue.sync_statuses;
    libraryTransfers = transfers;
    accountStorageUsage = storageUsage;
    localLibraryStorageUsage = localStorageUsage;
    serverLibraryStorageUsage = serverStorageUsage;
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

  /**
   * The account page's primary action: reconcile the catalogue and re-read the
   * transfer and storage state, so the verdict at the top of the page is the
   * one the user just asked for.
   */
  const syncAccountNow = () => runAccountRequest(ACCOUNT_CONTRACT.summary.syncing_label, "", () => loadServerTransferState());

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

  const registerAccount = (email: string, password: string) => runAccountRequest("Creating account…", "Check your email for the 6-digit verification PIN.", () => commands.requestPublicRegistration(email, password));
  const resendAccountVerification = (email: string) => runAccountRequest("Sending verification PIN…", "If the registration is pending, a new PIN has been sent.", () => commands.resendVerification(email));
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
      localLibraryStorageUsage = [];
      accountStorageUsage = null;
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

  async function removeLibraryFromDevice(libraryId: string) {
    const library = libraries.find((entry) => entry.library_id === libraryId);
    if (!library) return;
    const confirmed = window.confirm(
      `Delete “${library.library_name}”?\n\nIts database and state will be removed from this device immediately. Server deletion will happen now when possible, or automatically after the next sign-in. Local book files, folders, and stored server blobs will be kept.`,
    );
    if (!confirmed) return;

    showLibrarySwitcher = false;
    loadingLibrary = true;
    libraryError = "";
    try {
      await commands.removeLibrary(libraryId);
      const remaining = sortedLibraries(await commands.getLibraries());
      libraries = remaining;
      librarySyncStatuses = librarySyncStatuses.filter((status) => status.library_id !== libraryId);
      libraryTransfers = libraryTransfers.filter((group) => group.library_id !== libraryId);
      localLibraryStorageUsage = localLibraryStorageUsage.filter((usage) => usage.library_id !== libraryId);
      if (remaining.length) {
        await selectLibrary(remaining[0].library_id);
      } else {
        libraryGeneration += 1;
        activeLibraryId = "";
        resetLibraryData();
        loadingLibrary = false;
        await navigate("/library", true);
      }
    } catch (reason) {
      libraryError = `Could not remove library: ${errorMessage(reason)}`;
      loadingLibrary = false;
    }
  }

  async function openFolder(folder: LibraryDirectory) {
    clearLibrarySearch();
    await loadDirectory(folder.id, [...folderStack, { id: folder.id, name: folder.name }]);
    navigate(libraryRoute("folders", activeLibraryId, folder.id));
  }

  async function goToFolder(level: number) {
    clearLibrarySearch();
    const nextStack = folderStack.slice(0, level);
    const dirId = nextStack.at(-1)?.id ?? "__root__";
    await loadDirectory(dirId, nextStack);
    navigate(libraryRoute("folders", activeLibraryId, dirId));
  }

  async function createFolder(name: string) {
    const parentId = folderStack.at(-1)?.id ?? "__root__";
    const created = await measureInvoke("create_directory", () => commands.createDirectory(activeLibraryId, toBackendDirectoryId(parentId), name));
    const dirs = directoryContent.children.filter((folder) => folder.id !== created.id);
    directoryContent = {
      ...directoryContent,
      children: [...dirs, created].sort((left, right) => naturalCompare(left.name, right.name)),
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

  function clearLibrarySearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchQuery = "";
    titleSearchResults = [];
    authorSearchResults = [];
    searchCompletedQuery = "";
    searchCompletedScope = "";
    searchPending = false;
  }

  function focusContextualSearch() {
    if (activeDestination !== "folders" && activeDestination !== "subjects") setDestination("folders");
    requestAnimationFrame(() => document.querySelector<HTMLInputElement>("#contextual-library-search")?.focus());
  }

  function scheduleSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    const query = searchQuery.trim();
    if (!query) {
      titleSearchResults = [];
      authorSearchResults = [];
      searchCompletedQuery = "";
      searchCompletedScope = "";
      searchPending = false;
      return;
    }
    searchPending = true;
    searchTimer = setTimeout(() => void runSearch(query), SEARCH_DEBOUNCE_MS);
  }

  async function runSearch(query: string) {
    const scope = activeDestination === "folders"
      ? `folder:${folderStack.at(-1)?.id ?? "__root__"}`
      : activeDestination === "subjects"
        ? `subject:${subjectStack.at(-1)?.id ?? "__root__"}`
        : "global";
    try {
      const results = await measureInvoke("search", () => scope.startsWith("folder:")
        ? commands.searchLibraryFolder(activeLibraryId, toBackendDirectoryId(scope.slice("folder:".length)), query, BROWSING_CONTENT_CONTRACT.search_result_limit)
        : scope.startsWith("subject:")
          ? commands.searchLibrarySubject(activeLibraryId, scope === "subject:__root__" ? null : scope.slice("subject:".length), query, BROWSING_CONTENT_CONTRACT.search_result_limit)
          : commands.searchLibrary(activeLibraryId, query, BROWSING_CONTENT_CONTRACT.search_result_limit));
      const currentScope = activeDestination === "folders"
        ? `folder:${folderStack.at(-1)?.id ?? "__root__"}`
        : activeDestination === "subjects"
          ? `subject:${subjectStack.at(-1)?.id ?? "__root__"}`
          : "global";
      if (query === searchQuery.trim() && scope === currentScope) {
        await commitAndMeasureRender("search", () => {
          titleSearchResults = results.title_matches;
          authorSearchResults = results.author_matches;
        }, results.title_matches.length + results.author_matches.length);
        searchCompletedQuery = query;
        searchCompletedScope = scope;
        mergeBookDownloadStates([...results.title_matches, ...results.author_matches]);
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
    openingBookId = book.content_hash;
    libraryError = "";
    try {
      if ((bookDownloadStates[book.content_hash] ?? (book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded")) !== "Downloaded") {
        pendingOpenBook = book;
        await measureInvoke("download_book", () => downloadBook(book));
        if (bookDownloadStates[book.content_hash] !== "Downloaded") {
          return;
        }
      }
      pendingOpenBook = null;
      const openedBook = await openDownloadedBook(book.content_hash, book.title || "Untitled", book.author);
      readerBook = openedBook;
      navigateToReader(book.content_hash, openedBook);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      openingBookId = "";
    }
  }

  async function openDownloadedBook(bookId: string, title?: string, author?: string): Promise<ReaderBook> {
      const libraryId = activeLibraryId;
      if (!libraryId) throw new Error("No library is selected");
      const [localPath, fileName, initialCfi] = await Promise.all([
        measureInvoke("open_book", () => commands.openBook(libraryId, bookId)),
        commands.getBookFileName(libraryId, bookId),
        measureInvoke("reading_position", () => commands.getBookReadingPosition(libraryId, bookId)),
      ]);
      const resolvedTitle = title || fileName.replace(/\.[^.]+$/, "") || "Untitled";
      return {
        key: `library:${libraryId}:${bookId}`,
        bookId,
        file: convertFileSrc(localPath),
        fileName,
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
      const knownBook = [...recentlyAdded, ...recentlyRead, ...directoryContent.books, ...authorBooks, ...titleSearchResults, ...authorSearchResults]
        .find((book) => book.content_hash === bookId);
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
      await measureInvoke("import_book", () => commands.importBook({ library_id: activeLibraryId, directory_id: toBackendDirectoryId(parentId) }, sourcePath, picked.name));
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
    let unlistenCatalogueChanged: (() => void) | undefined;
    let transferRefreshTimer: ReturnType<typeof setTimeout> | undefined;
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
        try {
          const session = readUiSession();
          await loadAccountStatus();
          await loadLibraries();
          await restoreUiSession(session);
          routesReady = true;
          if (accountStatus.signed_in) {
            void loadServerTransferState().catch((reason) => {
              accountError = errorMessage(reason);
            });
          }
          await loadBrowsingPreferences();
          await loadReaderPreferences();
          await loadTransferPreferences();
        } finally {
          routesReady = true;
        }
      })();
    }
    void listen<DownloadStatusEvent>("library:download-status", ({ payload }) => {
      if (payload.library_id !== activeLibraryId) return;
      const change = payload.event.DownloadStatusChanged;
      bookDownloadStates = { ...bookDownloadStates, [change.content_hash]: change.state };
      if (pendingOpenBook?.content_hash === change.content_hash && change.state === "Downloaded" && openingBookId !== change.content_hash) {
        const book = pendingOpenBook;
        pendingOpenBook = null;
        queueMicrotask(() => void openLibraryBook(book));
      } else if (pendingOpenBook?.content_hash === change.content_hash && change.state === "NotDownloaded") {
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
    void listen("library:catalogue-changed", () => {
      if (!accountStatus.signed_in) return;
      void loadServerTransferState().catch((reason) => (accountError = errorMessage(reason)));
    }).then((unlisten) => {
      if (disposed) unlisten();
      else unlistenCatalogueChanged = unlisten;
    }).catch(() => undefined);
    const handleShortcut = (event: KeyboardEvent) => {
      if (isWebApp || readerBook || !(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
      const destination = ({ "1": "home", "2": "folders", "3": "subjects", "4": "authors", "5": "store", "6": "account", "7": "settings" } as const)[event.key];
      if (destination) {
        event.preventDefault();
        setDestination(destination);
      } else if (event.key.toLocaleLowerCase() === "f") {
        event.preventDefault();
        focusContextualSearch();
      }
    };
    document.addEventListener("keydown", handleShortcut);
    return () => {
      disposed = true;
      unlistenDownloadStatus?.();
      unlistenFolderDownloadStatus?.();
      unlistenTransferStatus?.();
      unlistenCatalogueChanged?.();
      if (transferRefreshTimer) clearTimeout(transferRefreshTimer);
      document.removeEventListener("keydown", handleShortcut);
      unobserveNetwork();
      if (searchTimer) clearTimeout(searchTimer);
      clearThumbnailCache();
    };
  });
</script>

<svelte:head><title>{isWebApp && !accountStatus.signed_in ? `Account · ${APP_NAME}` : readerBook ? `${readerBook.title} · Reader` : `${destinationTitle} · ${APP_NAME}`}</title></svelte:head>

{#snippet searchResults()}
  <div class="scoped-search-results" aria-live="polite">
    {#if searchPending}
      <div class="inline-empty">Searching…</div>
    {:else if searchCompletedQuery}
      {#if visibleTitleSearchResults.length}
        <h2>Title matches</h2>
        <BookGrid
          libraryId={activeLibraryId}
          books={visibleTitleSearchResults}
          detailed={browsingPreferences.library_view === "detailed"}
          emptyMessage=""
          {openingBookId}
          downloadStates={bookDownloadStates}
          onopen={openLibraryBook}
          ondownload={downloadBook}
          onremove={removeLocalBookCopy}
        />
      {/if}
      {#if visibleAuthorSearchResults.length}
        <h2>Author matches</h2>
        <BookGrid
          libraryId={activeLibraryId}
          books={visibleAuthorSearchResults}
          detailed={browsingPreferences.library_view === "detailed"}
          emptyMessage=""
          {openingBookId}
          downloadStates={bookDownloadStates}
          onopen={openLibraryBook}
          ondownload={downloadBook}
          onremove={removeLocalBookCopy}
        />
      {/if}
      {#if !visibleTitleSearchResults.length && !visibleAuthorSearchResults.length}
        <div class="inline-empty">{BROWSING_STATES_CONTRACT.no_search_results_label}</div>
      {/if}
    {/if}
  </div>
{/snippet}

{#snippet contextualSearch(placeholder: string)}
  <label class="search-field contextual-search">
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
    <input id="contextual-library-search" type="search" bind:value={searchQuery} oninput={scheduleSearch} {placeholder} aria-label={placeholder} autocomplete="off" />
  </label>
{/snippet}

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
      applicationAppearance={resolveAppearance(browsingPreferences.appearance)}
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
        folders={activeDestination === "subjects" ? subjectStack : folderStack}
        showBreadcrumb={activeDestination === "folders" || activeDestination === "subjects"}
        showLibraryActions={activeDestination === "folders"}
        {addingBook}
        accountSignedIn={accountStatus.signed_in}
        onlibraries={openLibrarySwitcher}
        onaccount={() => setDestination("account")}
        onsettings={() => setDestination("settings")}
        onaddbook={addBookToCurrentFolder}
        onnewfolder={() => (showCreateFolder = true)}
        onfolder={activeDestination === "subjects" ? goToSubject : goToFolder}
      />

      <main class:library-view={activeDestination === "folders" || activeDestination === "subjects"} class:authors-view={activeDestination === "authors"} class:store-view={activeDestination === "store"} class="content">
        {#if activeDestination === "account"}
          {#if !accountResolved}
            <section class="account-status-state" aria-live="polite"><span class="loader"></span><strong>Checking your session</strong></section>
          {:else if accountStatusError}
            <section class="account-status-state" role="alert">
              <strong>Account status unavailable</strong>
              <p>{accountStatusError}</p>
              <button type="button" onclick={() => void refreshAccountView()}>Retry</button>
            </section>
          {:else}
            <ServerAccountDialog
              account={accountStatus}
              {accountBusy}
              {accountProgress}
              {accountError}
              storageUsage={accountStorageUsage}
              {librarySyncStatuses}
              {libraryTransfers}
              {localLibraryStorageUsage}
              {serverLibraryStorageUsage}
              onrefresh={syncAccountNow}
              onlogin={loginToServer}
              onlogout={logoutFromServer}
              onregister={registerAccount}
              onresendverification={resendAccountVerification}
              onverifyemail={verifyAccountEmail}
              onrequestpasswordreset={requestAccountPasswordReset}
              onresetpassword={resetAccountPassword}
            />
          {/if}
        {:else if activeDestination === "settings"}
          <BrowsingSettingsDialog
            preferences={browsingPreferences}
            {networkStatus}
            {transferPreferences}
            serverConnected={accountStatus.signed_in && networkStatus.connected && networkStatus.validated !== false}
            error={settingsError}
            onchange={saveBrowsingPreference}
            ontransferpreference={saveTransferPreference}
          />
        {:else if activeDestination === "store" && !activeLibraryId}
          <EmptyPage
            title={BROWSING_STATES_CONTRACT.no_libraries_title}
            description={BROWSING_STATES_CONTRACT.no_libraries_description}
            primaryLabel={BROWSING_STATES_CONTRACT.create_library_label}
            onprimary={() => setDestination("folders")}
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
        {:else if activeDestination === "folders"}
          <section class="content-section library-section">
            {@render contextualSearch(folderStack.length ? `Search books or authors in ${folderStack.at(-1)?.name} and subfolders` : "Search books or authors")}
            {#if searchQuery.trim()}
              {@render searchResults()}
            {:else}
              {#if directoryContent.children.length}
                <div class="folder-grid">
                  {#each directoryContent.children.filter((folder) => folder.id !== "__root__") as folder}
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
                detailed={browsingPreferences.library_view === "detailed"}
                emptyMessage=""
                {openingBookId}
                downloadStates={bookDownloadStates}
                onopen={openLibraryBook}
                ondownload={downloadBook}
                onremove={removeLocalBookCopy}
              />
              {#if !directoryContent.children.length && !visibleDirectoryBooks.length}
                <EmptyPage
                  title={folderStack.length ? BROWSING_STATES_CONTRACT.empty_folder_title : BROWSING_STATES_CONTRACT.empty_library_title}
                  description={folderStack.length ? BROWSING_STATES_CONTRACT.empty_folder_description : BROWSING_STATES_CONTRACT.empty_library_description}
                  primaryLabel="Add book"
                  secondaryLabel="New folder"
                  onprimary={addBookToCurrentFolder}
                  onsecondary={() => { showCreateFolder = true; }}
                />
              {/if}
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
                  detailed={browsingPreferences.library_view === "detailed"}
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
        {:else if activeDestination === "subjects"}
          <section class="content-section library-section" aria-label="Subjects">
            {@render contextualSearch(subjectStack.length ? `Search books or authors in ${subjectStack.at(-1)?.name} and subsubjects` : "Search books or authors in all subjects")}
            {#if searchQuery.trim()}
              {@render searchResults()}
            {:else if subjectsLoading}
              <section class="page-state" aria-live="polite"><span class="loader"></span><strong>Loading subjects…</strong></section>
            {:else}
              {#if subjectContent.children.length}
                <div class="subject-grid">
                  {#each subjectContent.children as subject}
                    <button type="button" onclick={() => void openSubject(subject)}>
                      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20.6 13.6 13.7 20.5a2 2 0 0 1-2.8 0L3.5 13.1a2 2 0 0 1 0-2.8V4h6.3a2 2 0 0 1 1.4.6l9.4 9a1.4 1.4 0 0 1 0 2zM7.2 7.2h.1"></path></svg>
                      <span>{subject.name}</span>
                      <small>{subject.book_count}</small>
                    </button>
                  {/each}
                </div>
              {/if}
              <BookGrid
                libraryId={activeLibraryId}
                books={visibleSubjectBooks}
                detailed={browsingPreferences.library_view === "detailed"}
                emptyMessage=""
                {openingBookId}
                downloadStates={bookDownloadStates}
                onopen={openLibraryBook}
                ondownload={downloadBook}
                onremove={removeLocalBookCopy}
              />
              {#if !subjectContent.children.length && !visibleSubjectBooks.length}
                <EmptyPage title="No subjects" description="No books have subject information here yet." />
              {/if}
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
        onremove={removeLibraryFromDevice}
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
  .account-status-state { display: grid; width: min(100%, 36rem); margin: 2rem auto; padding: 1.25rem; justify-items: center; gap: .7rem; color: var(--color-subtle-text); text-align: center; }
  .account-status-state p { margin: 0; overflow-wrap: anywhere; font-size: .75rem; }
  .account-status-state button { min-height: 2.5rem; padding: .5rem .9rem; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-raised-bg); background: var(--color-accent); font: inherit; font-weight: 700; }

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
    padding: 1rem var(--page-gutter) calc(4.9rem + env(safe-area-inset-bottom));
    padding-right: max(var(--page-gutter), env(safe-area-inset-right));
    padding-left: max(var(--page-gutter), env(safe-area-inset-left));
    overflow-y: scroll;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    scrollbar-color: var(--color-border) transparent;
    scrollbar-width: thin;
  }
  .content::-webkit-scrollbar { width: .55rem; }
  .content::-webkit-scrollbar-track { background: transparent; }
  .content::-webkit-scrollbar-thumb { border: .14rem solid transparent; border-radius: 999px; background: var(--color-border); background-clip: padding-box; }
  .content.library-view { padding-top: 0; }

  .section-heading h2 {
    margin: 0;
    font-family: var(--font-interface-family);
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
  .loader { width: 1.4rem; height: 1.4rem; border: .18rem solid var(--color-border); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .inline-empty { display: grid; place-items: center; min-height: 8rem; color: var(--color-subtle-text); font-size: .84rem; text-align: center; }

  .library-section { margin-top: 0; }
  .folder-grid, .subject-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 13rem), 1fr)); margin: -.125rem -.125rem 1.125rem; }
  .subject-grid button { display: grid; grid-template-columns: 1.3rem minmax(0, 1fr) auto; align-items: center; gap: .65rem; min-height: 2.6rem; margin: .125rem; padding: .55rem .75rem; border: 0; border-radius: 0; color: inherit; background: var(--color-raised-bg); text-align: left; }
  .subject-grid button:active { transform: translateY(1px); }
  .subject-grid svg { width: 1.3rem; color: var(--color-accent); }
  .subject-grid span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 17px; font-weight: 500; }
  .subject-grid small { color: var(--color-subtle-text); font-size: .8125rem; font-weight: 650; text-align: right; }

  .content.authors-view { display: flex; flex-direction: column; overflow: hidden; }
  .authors-layout { display: grid; flex: 1; width: min(1200px, 100%); min-height: 0; margin: 0 auto; gap: 2rem; overflow: hidden; }
  .author-list-panel { display: flex; flex-direction: column; min-height: 0; }
  .author-list-panel.has-selection { display: none; }
  .author-books-panel { display: none; min-height: 0; overflow-y: auto; }
  .author-list-panel.has-selection + .author-books-panel { display: block; }
  .author-heading { align-items: start; justify-content: start; }
  .author-back { display: inline-flex; align-items: center; gap: .35rem; padding: .45rem .55rem; border: 1px solid var(--color-border); border-radius: .65rem; color: var(--color-accent); background: color-mix(in srgb, var(--color-raised-bg) 58%, transparent); font-size: .72rem; font-weight: 750; }
  .author-filter { display: grid; flex: none; grid-template-columns: auto minmax(0, 1fr); align-items: center; gap: .6rem; min-height: 2.9rem; margin-bottom: .8rem; padding: 0 .75rem; border: 1px solid var(--color-border); border-radius: .65rem; background: var(--color-raised-bg); }
  .author-filter:focus-within { border-color: var(--color-accent); }
  .author-filter svg { width: 1.05rem; color: var(--color-accent); }
  .author-filter input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font: inherit; font-size: .82rem; }
  .author-list-scroll { flex: 1; min-height: 0; padding-right: .2rem; overflow-y: auto; overscroll-behavior-y: contain; scrollbar-color: var(--color-border) transparent; scrollbar-width: thin; }
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
  .author-list button.active { border-color: var(--color-border); background: color-mix(in srgb, var(--color-raised-bg) 60%, transparent); }
  .author-list button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: -2px; }
  .author-list button > span:nth-child(2) { display: grid; min-width: 0; gap: .12rem; }
  .author-list strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: 500 .95rem var(--font-interface-family); }
  .author-list small { color: var(--color-subtle-text); font-size: .68rem; }
  .author-list i { color: var(--color-border); font-style: normal; }
  .author-avatar { display: grid; place-items: center; width: 2.65rem; aspect-ratio: 1; border-radius: 50%; color: var(--color-raised-bg); background: var(--color-accent); font: 500 1rem var(--font-interface-family); }

  .search-field { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: center; gap: .7rem; min-height: 3.6rem; padding: 0 1rem; border: 1px solid var(--color-border); border-radius: 1rem; background: color-mix(in srgb, var(--color-raised-bg) 70%, transparent); box-shadow: 0 .7rem 2rem color-mix(in srgb, var(--color-box-shadow) 20%, transparent); }
  .search-field svg { width: 1.25rem; color: var(--color-accent); }
  .search-field input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font-size: 1rem; }
  .contextual-search { margin-bottom: 1rem; }
  .scoped-search-results { display: grid; gap: .75rem; }
  .scoped-search-results h2 { margin: .35rem 0 0; font: 600 1rem var(--font-interface-family); }

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
  .adaptive-shell { background: var(--color-surface-bg); }
  .content { max-width: none; padding-top: 1.1rem; }
  .content-section { margin-top: 1.8rem; }
  .section-heading { margin-bottom: .8rem; }
  .section-heading h2 { font-size: 1.35rem; letter-spacing: -.02em; }
  .inline-empty { min-height: 5rem; justify-items: start; text-align: left; }
  .search-field { border-radius: .65rem; box-shadow: none; }
  .author-list button { border-radius: .55rem; }
  .author-list button.active { background: var(--color-active-highlight); }

</style>
