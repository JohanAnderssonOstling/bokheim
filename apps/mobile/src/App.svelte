<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { listen } from "./lib/app-events";

  import Reader from "./components/Reader.svelte";
  import PdfReader from "./components/PdfReader.svelte";
  import AudiobookPlayer from "./components/AudiobookPlayer.svelte";
  import LibraryHost from "./components/library/LibraryHost.svelte";
  import BookDetailPage from "./components/library/BookDetailPage.svelte";
  import FolderPickerDialog from "./components/library/FolderPickerDialog.svelte";
  import BrowserRoot from "./components/shell/BrowserRoot.svelte";
  import UpdateNotice from "./components/shell/UpdateNotice.svelte";
  import UniversalHost from "./components/universal/UniversalHost.svelte";
  import { applyApplicationAppearance, observeSystemAppearance, resolveAppearance } from "./lib/application-appearance";
  import { commands } from "./lib/commands";
  import { toBackendDirectoryId } from "./lib/directory-id";
  import { pickEpub, type PickedEpub } from "./lib/epub-picker";
  import { importFolderArchive, importLibraryArchive, pickLibraryArchive } from "./lib/web/library-archive";
  import { APP_NAV_ITEMS, libraryReaderBookKey, NAV_ITEMS, readerRouteCacheKey, type Destination, universalDestinationForRoute } from "./lib/navigation";
  import { observeNetworkStatus, UNKNOWN_NETWORK_STATUS } from "./lib/network-status";
  import { commitAndMeasureRender, measureInvoke } from "./lib/performance";
  import { ScreenTransitionController, type ScreenTransitionState } from "./lib/screen-transition";
  import { clearThumbnailCache, getThumbnailUrl } from "./lib/thumbnail-cache";
  import type { MobileAudiobookSession } from "./lib/audiobook";
  import { nativeAudiobookAvailable, observeNativeAudiobookState, type NativeAudiobookState } from "./lib/native-audiobook";
  import { defaultLibraryPlace, readUiSession, writeUiSession, type AppUiSession } from "./lib/ui-session";
  import { BrowserSessionStore } from "./lib/stores/browser-session.svelte";
  import { LibraryStateStore } from "./lib/stores/library-state.svelte";
  import type { BookDetail, LibraryFeature, LibraryFileTypeFilter, LibraryFolderDestination, LibraryTrashView, TrashedBook, TrashedFolder } from "./lib/bindings";
  import {
    ACCOUNT_CONTRACT,
    BROWSING_CONTENT_CONTRACT,
    BROWSING_LAYOUT_CONTRACT,
    BROWSING_STATES_CONTRACT,
    DEFAULT_READER_PREFERENCES,
    DEFAULT_BROWSING_PREFERENCES,
    type AccountStatus,
    type AccountStorageUsage,
    type AuthorSort,
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
  import { apkUpdaterAvailable, checkForApkUpdate, EMPTY_APK_UPDATE_STATE, installApkUpdate, observeApkUpdates, openApkInstallSettings, type ApkUpdateState } from "./lib/apk-updater";

  type FolderLevel = { id: string; name: string };
  type FolderPickerOperation =
    | { kind: "copy_book"; book: BookCardData; sourceId: string }
    | { kind: "move_book"; book: BookCardData; sourceId: string }
    | { kind: "move_folder"; folder: LibraryDirectory; sourceParentId: string };
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
  const WEB_BROWSING_PREFERENCES_KEY = `${APP_NAME_LOWER}-browsing-preferences`;

  function isPdfReaderBook(book: ReaderBook) {
    const source = book.file;
    if (typeof File !== "undefined" && source instanceof File) {
      return source.type === "application/pdf" || /\.pdf$/i.test(source.name);
    }
    const path = book.fileName ?? (typeof source === "string" ? source : "");
    return /\.pdf(?:$|[?#])/i.test(path);
  }

  function loadWebBrowsingPreferencesFallback(): BrowsingPreferences | null {
    if (!isWebApp) return null;
    try {
      const saved = localStorage.getItem(WEB_BROWSING_PREFERENCES_KEY);
      return saved ? { ...DEFAULT_BROWSING_PREFERENCES, ...JSON.parse(saved) } : null;
    } catch {
      return null;
    }
  }

  function saveWebBrowsingPreferencesFallback(preferences: BrowsingPreferences) {
    if (!isWebApp) return;
    try {
      localStorage.setItem(WEB_BROWSING_PREFERENCES_KEY, JSON.stringify(preferences));
    } catch {
      // Browsers may deny storage in private or restricted contexts. The
      // worker-backed database remains the preferred persistence path.
    }
  }

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
    const routeLibraryId = page.params.libraryId || libraryId || activeLibraryId || undefined;
    let route: LibraryFeature | undefined;
    if (routeId === "/library/[libraryId]/folder/[folderId]") route = "folders";
    else if (routeId === "/library/[libraryId]/authors") route = "authors";
    else if (routeId === "/library/[libraryId]/subjects/[subjectId]") route = "subjects";
    else if (routeId === "/library/[libraryId]/trash") route = "trash";
    else if (routeId === "/library/[libraryId]") route = "home";
    if (routeLibraryId && route) {
      browserSession.remember(routeLibraryId, route, folderStack, subjectStack);
    }
    return browserSession.snapshot(routeLibraryId);
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
    `--layout-sidebar-width: max(${BROWSING_LAYOUT_CONTRACT.sidebar_width_px}px, ${BROWSING_LAYOUT_CONTRACT.sidebar_width_px / 16}rem)`,
    `--navigation-label-size: ${BROWSING_LAYOUT_CONTRACT.navigation_label_size_px / 16}rem`,
    `--navigation-label-weight: ${BROWSING_LAYOUT_CONTRACT.navigation_label_weight}`,
    `--navigation-icon-size: ${BROWSING_LAYOUT_CONTRACT.navigation_icon_size_px / 16}rem`,
    `--navigation-icon-label-gap: ${BROWSING_LAYOUT_CONTRACT.navigation_icon_label_gap_px}px`,
    `--navigation-item-min-height: ${BROWSING_LAYOUT_CONTRACT.navigation_item_min_height_px}px`,
    `--navigation-touch-item-min-height: ${BROWSING_LAYOUT_CONTRACT.navigation_touch_item_min_height_px}px`,
    `--layout-topbar-height: ${BROWSING_LAYOUT_CONTRACT.topbar_height_px}px`,
    `--layout-topbar-action-height: ${BROWSING_LAYOUT_CONTRACT.topbar_action_height_px}px`,
    `--topbar-action-label-weight: ${BROWSING_LAYOUT_CONTRACT.topbar_action_label_weight}`,
    `--layout-topbar-action-gap: ${BROWSING_LAYOUT_CONTRACT.topbar_action_gap_px}px`,
    `--layout-topbar-action-horizontal-padding: ${BROWSING_LAYOUT_CONTRACT.topbar_action_horizontal_padding_px}px`,
    `--layout-content-padding: ${BROWSING_LAYOUT_CONTRACT.content_padding_px}px`,
    `--surface-corner-radius: ${BROWSING_LAYOUT_CONTRACT.surface_corner_radius_px}px`,
    `--book-cover-width: ${BROWSING_LAYOUT_CONTRACT.book_cover_width_px}px`,
    `--book-cover-height: ${BROWSING_LAYOUT_CONTRACT.book_cover_height_px}px`,
    `--book-card-height: ${BROWSING_LAYOUT_CONTRACT.book_card_height_px}px`,
    `--book-grid-column-gap: ${BROWSING_LAYOUT_CONTRACT.book_grid_column_gap_px}px`,
    `--book-grid-row-height: ${BROWSING_LAYOUT_CONTRACT.book_grid_row_height_px}px`,
    `--book-card-title-size: ${BROWSING_LAYOUT_CONTRACT.book_card_title_size_px / 16}rem`,
    `--book-card-title-weight: ${BROWSING_LAYOUT_CONTRACT.book_card_title_weight}`,
    `--book-card-title-line-height: ${BROWSING_LAYOUT_CONTRACT.book_card_title_line_height_px / BROWSING_LAYOUT_CONTRACT.book_card_title_size_px}`,
    `--book-card-title-block-height: ${(BROWSING_LAYOUT_CONTRACT.book_card_title_line_height_px * BROWSING_LAYOUT_CONTRACT.book_card_title_lines) / 16}rem`,
    `--book-card-title-lines: ${BROWSING_LAYOUT_CONTRACT.book_card_title_lines}`,
    `--book-card-author-size: ${BROWSING_LAYOUT_CONTRACT.book_card_author_size_px / 16}rem`,
    `--book-card-author-line-height: ${BROWSING_LAYOUT_CONTRACT.book_card_author_line_height_px / BROWSING_LAYOUT_CONTRACT.book_card_author_size_px}`,
    `--book-card-author-block-height: ${BROWSING_LAYOUT_CONTRACT.book_card_author_line_height_px / 16}rem`,
    `--book-card-cover-content-gap: ${BROWSING_LAYOUT_CONTRACT.book_card_cover_content_gap_px}px`,
    `--book-card-text-gap: ${BROWSING_LAYOUT_CONTRACT.book_card_text_gap_px}px`,
    `--book-card-progress-height: ${BROWSING_LAYOUT_CONTRACT.book_card_progress_height_px}px`,
    `--book-grid-row-trailing-space: ${BROWSING_LAYOUT_CONTRACT.book_grid_row_trailing_space_px}px`,
    `--carousel-title-size: ${BROWSING_LAYOUT_CONTRACT.carousel_title_size_px / 16}rem`,
    `--carousel-title-weight: ${BROWSING_LAYOUT_CONTRACT.carousel_title_weight}`,
    `--carousel-section-gap: ${BROWSING_LAYOUT_CONTRACT.carousel_section_gap_px}px`,
    `--carousel-content-gap: ${BROWSING_LAYOUT_CONTRACT.carousel_content_gap_px}px`,
    `--carousel-control-gap: ${BROWSING_LAYOUT_CONTRACT.carousel_control_gap_px}px`,
    `--carousel-arrow-size: ${BROWSING_LAYOUT_CONTRACT.carousel_arrow_size_px}px`,
    `--author-row-cover-width: ${BROWSING_LAYOUT_CONTRACT.author_row_cover_width_px}px`,
    `--author-row-cover-height: ${BROWSING_LAYOUT_CONTRACT.author_row_cover_height_px}px`,
    `--author-row-height: ${BROWSING_LAYOUT_CONTRACT.author_row_height_px}px`,
    `--author-row-cover-gap: ${BROWSING_LAYOUT_CONTRACT.author_row_cover_gap_px}px`,
    `--author-row-content-gap: ${BROWSING_LAYOUT_CONTRACT.author_row_content_gap_px}px`,
    `--author-row-horizontal-padding: ${BROWSING_LAYOUT_CONTRACT.author_row_horizontal_padding_px}px`,
    `--author-row-font-size: ${BROWSING_LAYOUT_CONTRACT.author_row_label_size_px / 16}rem`,
    `--hierarchy-touch-cell-min-height: ${BROWSING_LAYOUT_CONTRACT.hierarchy_touch_cell_min_height_px}px`,
    `--hierarchy-cell-min-width: ${BROWSING_LAYOUT_CONTRACT.hierarchy_cell_min_width_px}px`,
    `--hierarchy-cell-gap: ${BROWSING_LAYOUT_CONTRACT.hierarchy_cell_gap_px}px`,
    `--hierarchy-icon-size: ${BROWSING_LAYOUT_CONTRACT.hierarchy_icon_size_px}px`,
    `--hierarchy-label-size: ${BROWSING_LAYOUT_CONTRACT.hierarchy_label_size_px / 16}rem`,
  ].join("; ");

  const libraryState = new LibraryStateStore();
  const libraries = $derived(libraryState.entries);
  const activeLibraryId = $derived(libraryState.activeLibraryId);
  let activeDestination = $state<Destination>("home");
  const browserSession = new BrowserSessionStore();
  let recentlyAdded = $state<BookCardData[]>([]);
  let recentlyRead = $state<BookCardData[]>([]);
  const recentlyInProgress = $derived(recentlyRead.filter((book) => (book.progress ?? 0) < BROWSING_CONTENT_CONTRACT.finished_progress_threshold));
  const recentlyFinished = $derived(recentlyRead.filter((book) => (book.progress ?? 0) >= BROWSING_CONTENT_CONTRACT.finished_progress_threshold));
  let directoryContent = $state<DirectoryContent>(EMPTY_DIRECTORY);
  let folderStack = $state<FolderLevel[]>([]);
  let subjectContent = $state<DirectoryContent>(EMPTY_DIRECTORY);
  let subjectStack = $state<FolderLevel[]>([]);
  let hierarchyFileType = $state<LibraryFileTypeFilter>("all");
  let subjectsLoading = $state(false);
  let trash = $state<LibraryTrashView>({ books: [], folders: [] });
  let trashLoading = $state(false);
  let trashBusy = $state(false);
  let trashError = $state("");
  let subjectsLoadedFor = $state("");
  let subjectRequest = 0;
  let authors = $state<AuthorSummary[]>([]);
  // Filing order groups the list under letters; book count does not, so the
  // dividers go with it.
  let authorSort = $state<AuthorSort>("name");
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
  let folderPickerOperation = $state<FolderPickerOperation | null>(null);
  let folderPickerDestinations = $state<LibraryFolderDestination[]>([]);
  let applicationSurface = $state<ScreenTransitionState<ReaderBook>>({ phase: "browser", reader: null });
  const screenTransitions = new ScreenTransitionController<ReaderBook>((state) => (applicationSurface = state));
  let readerBook = $derived(applicationSurface.reader);
  let audiobookSession = $state<MobileAudiobookSession | null>(null);
  let audiobookRequest = 0;
  let suppressNativeAudiobookRestore = false;
  let restoringNativeAudiobookKey = "";
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
  let bookDetail = $state<BookDetail | null>(null);
  let bookDetailLoading = $state(false);
  let bookDetailError = $state("");
  let bookDetailRequest = 0;
  const bookDetailScrollPositions = new Map<string, number>();
  let pendingOpenBook: { book: BookCardData; initialTarget?: string } | null = null;
  let bookDownloadStates = $state<Record<string, DownloadState>>({});
  let folderDownloadStatuses = $state<Record<string, FolderDownloadStatus>>({});
  let activeFolderDownloads = $state<Record<string, boolean>>({});
  let libraryError = $state("");
  let creatingLibrary = $state(false);
  let addingBook = $state(false);
  let importingLibraryArchive = $state(false);
  let importingFolderArchive = $state(false);
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
  let routesReady = $state(false);
  let routeSynchronization = 0;
  let webForegroundSync: Promise<void> | undefined;
  let lastWebForegroundSyncAt = 0;
  let apkUpdateState = $state<ApkUpdateState>({ ...EMPTY_APK_UPDATE_STATE });
  let dismissedUpdateVersion = $state("");
  const ACCOUNT_STATUS_TIMEOUT_MS = isWebApp ? 10_000 : 2_000;
  const showApkUpdateNotice = $derived(
    apkUpdateState.phase === "downloading" || apkUpdateState.phase === "permission_required" || apkUpdateState.phase === "installing" ||
    (apkUpdateState.phase === "available" && apkUpdateState.version !== dismissedUpdateVersion),
  );

  const destinationTitle = $derived(bookDetail?.book.title || [...NAV_ITEMS, ...APP_NAV_ITEMS].find((item) => item.id === activeDestination)?.label || APP_NAME);
  const activeLibrary = $derived(libraryState.active);
  const visibleDirectoryBooks = $derived(directoryContent.books);
  const visibleSubjectBooks = $derived(subjectContent.books);
  const visibleAuthorBooks = $derived(authorBooks);
  const visibleTitleSearchResults = $derived(titleSearchResults);
  const visibleAuthorSearchResults = $derived(authorSearchResults);
  const visibleRecentlyInProgress = $derived(recentlyInProgress);
  const visibleRecentlyFinished = $derived(recentlyFinished);
  const visibleRecentlyAdded = $derived(recentlyAdded);
  const filteredAuthors = $derived(
    authors.filter((author) => {
      const filter = authorFilter.trim().toLocaleLowerCase();
      // Both forms match: the reader may type either the name as printed or
      // the one the index files under.
      return !filter || author.name.toLocaleLowerCase().includes(filter) || author.sort_name.toLocaleLowerCase().includes(filter);
    }),
  );

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
    if (destination === "trash") return `${library}/trash`;
    return library;
  }

  async function navigate(path: string, replaceState = false) {
    if (`${page.url.pathname}${page.url.search}` !== path) await goto(path, { replaceState, noScroll: true, keepFocus: true });
  }

  async function closeReader() {
    const fallback = page.route.id === "/reader/[bookId]" ? libraryRoute("home", activeLibraryId) : "";
    const closing = screenTransitions.beginReaderClose(fallback);
    // The closing phase renders neither interactive surface. Waiting one flush
    // guarantees Reader cleanup releases its global listeners and Foliate view
    // before the route can mount the browser shell.
    await tick();
    try {
      if (closing.destination) await navigate(closing.destination, true);
    } finally {
      screenTransitions.finishReaderClose(closing.token);
    }
  }

  async function navigateToReader(bookId: string, book: ReaderBook) {
    const returnPath = page.route.id === "/reader/[bookId]" ? "" : `${page.url.pathname}${page.url.search}`;
    if (returnPath.includes("/book/")) {
      const scroll = document.querySelector<HTMLElement>(".content");
      if (scroll) bookDetailScrollPositions.set(returnPath, scroll.scrollTop);
    }
    // The route synchronizer runs as part of goto(). Give it the already-open
    // reader for this handoff so it cannot mistake a remote-only publication
    // for a missing local download. Every entry leaves this handoff cache as
    // soon as the transition completes; Reader owns the resulting lifetime.
    const cacheKey = readerRouteCacheKey(activeLibraryId, bookId);
    const transition = screenTransitions.beginReaderOpen(returnPath, cacheKey, book);
    const library = activeLibraryId ? `?library=${encodeURIComponent(activeLibraryId)}` : "";
    try {
      await navigate(`/reader/${encodeURIComponent(bookId)}${library}`);
      if (!screenTransitions.commitReader(transition, cacheKey, book)) throw new Error("Reader opening was superseded by a newer navigation");
    } catch (reason) {
      screenTransitions.cancelReaderOpen(transition);
      if (typeof book.file === "string" && book.file.startsWith("blob:")) URL.revokeObjectURL(book.file);
      if (book.file && typeof book.file === "object" && !(book.file instanceof File) && book.file.kind === "native_remote_epub") {
        await commands.closeRemoteEpub(book.file.sessionId).catch(() => undefined);
      }
      throw reason;
    } finally {
      screenTransitions.releaseHandoff(cacheKey, transition);
    }
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
    const synchronization = ++routeSynchronization;
    // synchronizeRoute reads application state synchronously before its first
    // await. Those reads must not make reader/library state into dependencies
    // of this effect; only the route snapshot above should trigger a new pass.
    untrack(() => void synchronizeRoute(route, synchronization));
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
  }, synchronization: number) {
    const current = () => synchronization === routeSynchronization;
    if (isWebApp && !accountResolved) return;
    if (isWebApp && !webAppReady) return;

    if (route.id === "/" || route.id === "/login" || route.id === "/library") {
      await navigate(libraryRoute("home"), true);
      return;
    }

    if (route.id === "/reader/[bookId]" && route.bookId) {
      if (route.readerLibraryId && route.readerLibraryId !== activeLibraryId && libraries.some((entry) => entry.library_id === route.readerLibraryId)) {
        await selectLibrary(route.readerLibraryId, false);
        if (!current()) return;
      }
      await restoreReaderRoute(route.bookId, current);
      return;
    }

    if (route.id === "/library/[libraryId]/book/[bookId]" && route.libraryId && route.bookId) {
      screenTransitions.showBrowser();
      if (route.libraryId !== activeLibraryId && libraries.some((entry) => entry.library_id === route.libraryId)) {
        await selectLibrary(route.libraryId, false);
        if (!current()) return;
      }
      await loadBookDetail(route.libraryId, route.bookId, current);
      return;
    }


    if (!current()) return;
    // Invalidate any detail request that belongs to the route we just left.
    // Its command may still finish, but it must not keep the browser shell in
    // detail mode or publish stale data into this destination.
    bookDetailRequest += 1;
    bookDetailLoading = false;
    bookDetail = null;
    bookDetailError = "";
    screenTransitions.showBrowser();
    const universalDestination = universalDestinationForRoute(route.id);
    if (universalDestination) {
      setDestination(universalDestination, false);
      return;
    }

    const destination: Destination = route.id === "/library/[libraryId]/subjects/[subjectId]"
      ? "subjects"
      : route.id === "/library/[libraryId]/authors"
      ? "authors"
      : route.id === "/library/[libraryId]/trash"
      ? "trash"
      : route.id === "/library/[libraryId]/folder/[folderId]"
          ? "folders"
          : "home";

    if (route.libraryId && route.libraryId !== activeLibraryId && libraries.some((entry) => entry.library_id === route.libraryId)) {
      await selectLibrary(route.libraryId, false);
      if (!current()) return;
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
      if (!current()) return;
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
    const libraryId = session.active_library_id;
    if (!libraryId) return "/library";
    const place = session.places[libraryId] ?? defaultLibraryPlace();
    const locationId = place.route === "folders" ? place.folder_path.at(-1)?.id : place.route === "subjects" ? place.subject_path.at(-1)?.id : undefined;
    return libraryRoute(place.route, libraryId, locationId);
  }

  async function restoreUiSession(session: AppUiSession | null) {
    if (!session) return false;
    if (session.kind === "browse") browserSession.restore(session);
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
      await openLibraryBook(pendingOpenBook.book, pendingOpenBook.initialTarget);
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
    if (activeDestination === "trash") await loadTrash();
    if (activeDestination === "account") await loadServerTransferState();
  }

  async function refreshVisibleWebLibraryState() {
    const entries = sortedLibraries(await commands.getLibraries());
    libraryState.setEntries(entries);
    if (!entries.some((entry) => entry.library_id === activeLibraryId)) {
      if (entries[0]) await selectLibrary(entries[0].library_id);
      return;
    }
    if (readerBook) return;
    clearThumbnailCache();
    const selectedAuthor = selectedAuthorId ? { id: selectedAuthorId, name: selectedAuthorName } : null;
    await retryActiveSection();
    if (activeDestination === "authors" && selectedAuthor) await selectAuthor(selectedAuthor.id, selectedAuthor.name);
  }

  function synchronizeForegroundWebApp(force = false): Promise<void> {
    if (!isWebApp || !webAppReady || !accountResolved || document.visibilityState === "hidden") return Promise.resolve();
    if (webForegroundSync) return webForegroundSync;
    if (!force && Date.now() - lastWebForegroundSyncAt < 30_000) return Promise.resolve();
    lastWebForegroundSyncAt = Date.now();
    webForegroundSync = (async () => {
      const status = await commands.getAccountStatus();
      accountStatus = status;
      if (!status.signed_in) return;
      await commands.syncAllLibraries();
      await refreshVisibleWebLibraryState();
    })().catch(() => undefined).finally(() => (webForegroundSync = undefined));
    return webForegroundSync;
  }

  function resetLibraryData() {
    folderPickerOperation = null;
    folderPickerDestinations = [];
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
    trash = { books: [], folders: [] };
    trashLoading = false;
    trashBusy = false;
    trashError = "";
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

  async function moveBookToTrash(book: BookCardData) {
    if (!window.confirm(`Move “${book.title || "Untitled"}” to Trash?`)) return;
    libraryError = "";
    try {
      await commands.moveBookToTrash(activeLibraryId, book.content_hash);
      homeLoadedFor = "";
      authorsLoadedFor = "";
      folderLoadedFor = "";
      subjectsLoadedFor = "";
      await retryActiveSection();
    } catch (reason) {
      libraryError = errorMessage(reason);
    }
  }

  async function trashDetailBook(book: BookCardData) {
    if (!window.confirm(`Move “${book.title || "Untitled"}” to Trash?`)) return;
    libraryError = "";
    try {
      await commands.moveBookToTrash(activeLibraryId, book.content_hash);
      homeLoadedFor = "";
      authorsLoadedFor = "";
      folderLoadedFor = "";
      subjectsLoadedFor = "";
      backFromBookDetail();
    } catch (reason) {
      libraryError = errorMessage(reason);
    }
  }

  async function moveFolderToTrash(folder: LibraryDirectory) {
    if (!window.confirm(`Move the folder “${folder.name}” and its contents to Trash?`)) return;
    libraryError = "";
    try {
      await commands.moveFolderToTrash(activeLibraryId, toBackendDirectoryId(folder.id));
      await loadDirectory(folderStack.at(-1)?.id ?? "__root__", folderStack, libraryGeneration, true);
    } catch (reason) {
      libraryError = errorMessage(reason);
    }
  }

  function currentFolderId(): string {
    return toBackendDirectoryId(folderStack.at(-1)?.id ?? "__root__");
  }

  async function refreshAfterFolderMutation() {
    homeLoadedFor = "";
    authorsLoadedFor = "";
    subjectsLoadedFor = "";
    folderLoadedFor = "";
    await loadDirectory(folderStack.at(-1)?.id ?? "__root__", folderStack, libraryGeneration, true);
  }

  async function openFolderPicker(operation: FolderPickerOperation) {
    libraryError = "";
    try {
      const destinations = operation.kind === "move_folder"
        ? await commands.getFolderDestinations(activeLibraryId, operation.folder.id, operation.sourceParentId)
        : await commands.getFolderDestinations(activeLibraryId, operation.sourceId, null);
      if (!destinations.length) throw new Error("There are no valid destination folders");
      folderPickerDestinations = destinations;
      folderPickerOperation = operation;
    } catch (reason) {
      libraryError = `Could not open folder picker: ${errorMessage(reason)}`;
    }
  }

  function copyBookToFolder(book: BookCardData) {
    return openFolderPicker({ kind: "copy_book", book, sourceId: currentFolderId() });
  }

  function moveBookToFolder(book: BookCardData) {
    return openFolderPicker({ kind: "move_book", book, sourceId: currentFolderId() });
  }

  function copyDetailBookToFolder(book: BookCardData) {
    if (!bookDetail?.source_directory_id) return Promise.resolve();
    return openFolderPicker({ kind: "copy_book", book, sourceId: bookDetail.source_directory_id });
  }

  function moveDetailBookToFolder(book: BookCardData) {
    if (!bookDetail?.source_directory_id) return Promise.resolve();
    return openFolderPicker({ kind: "move_book", book, sourceId: bookDetail.source_directory_id });
  }

  function chooseFolderMove(folder: LibraryDirectory) {
    return openFolderPicker({ kind: "move_folder", folder, sourceParentId: currentFolderId() });
  }

  async function confirmFolderPicker(destinationId: string) {
    const operation = folderPickerOperation;
    if (!operation) return;
    const detailHash = bookDetail?.book.content_hash;
    if (operation.kind === "copy_book") {
      await commands.copyBookToFolder(activeLibraryId, operation.book.content_hash, operation.sourceId, destinationId);
    } else if (operation.kind === "move_book") {
      await commands.moveBookToFolder(activeLibraryId, operation.book.content_hash, operation.sourceId, destinationId);
    } else {
      await commands.moveFolder(activeLibraryId, toBackendDirectoryId(operation.folder.id), destinationId);
    }
    await refreshAfterFolderMutation();
    if (detailHash) await loadBookDetail(activeLibraryId, detailHash);
  }

  function closeFolderPicker() {
    folderPickerOperation = null;
    folderPickerDestinations = [];
  }

  function folderPickerTitle(operation: FolderPickerOperation): string {
    if (operation.kind === "copy_book") return `Copy “${operation.book.title || "Untitled"}”`;
    if (operation.kind === "move_book") return `Move “${operation.book.title || "Untitled"}”`;
    return `Move “${operation.folder.name}”`;
  }

  async function renameFolder(folder: LibraryDirectory) {
    const requested = window.prompt(`Rename “${folder.name}”`, folder.name);
    const name = requested?.trim();
    if (!name || name === folder.name) return;
    libraryError = "";
    try {
      await commands.renameFolder(activeLibraryId, toBackendDirectoryId(folder.id), name);
      await refreshAfterFolderMutation();
    } catch (reason) {
      libraryError = `Could not rename folder: ${errorMessage(reason)}`;
    }
  }

  async function removeBookFromFolder(book: BookCardData) {
    if (!window.confirm(`Remove “${book.title || "Untitled"}” from this folder? If it has no other placement, it will move to Trash.`)) return;
    libraryError = "";
    try {
      await commands.removeBookFromFolder(activeLibraryId, book.content_hash, currentFolderId());
      await refreshAfterFolderMutation();
    } catch (reason) {
      libraryError = `Could not remove book from folder: ${errorMessage(reason)}`;
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
      if (folderLoadedFor !== `${activeLibraryId}:${dirId}:${hierarchyFileType}`) await loadDirectory(dirId, folderStack, generation);
    }
    if (destination === "subjects") {
      const subjectId = subjectStack.at(-1)?.id;
      const key = `${activeLibraryId}:${subjectId ?? "__root__"}:${hierarchyFileType}`;
      if (subjectsLoadedFor !== key) await loadSubjects(subjectId, subjectStack, generation);
    }
    if (destination === "authors" && authorsLoadedFor !== activeLibraryId) await loadAuthors(generation);
    if (destination === "trash") await loadTrash();
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

  async function loadTrash() {
    if (!activeLibraryId || trashLoading) return;
    const libraryId = activeLibraryId;
    trashLoading = true;
    trashError = "";
    try {
      const contents = await commands.getLibraryTrash(libraryId);
      if (libraryId === activeLibraryId) trash = contents;
    } catch (reason) {
      if (libraryId === activeLibraryId) trashError = `Could not load Trash: ${errorMessage(reason)}`;
    } finally {
      if (libraryId === activeLibraryId) trashLoading = false;
    }
  }

  async function runTrashAction(action: () => Promise<unknown>) {
    trashBusy = true;
    trashError = "";
    try {
      await action();
      await loadTrash();
    } catch (reason) {
      trashError = errorMessage(reason);
    } finally {
      trashBusy = false;
    }
  }

  function restoreTrashedBook(book: TrashedBook) {
    return runTrashAction(() => commands.restoreTrashedBook(activeLibraryId, book.content_hash));
  }

  function purgeTrashedBook(book: TrashedBook) {
    return runTrashAction(() => commands.purgeTrashedBook(activeLibraryId, book.content_hash));
  }

  function restoreTrashedFolder(folder: TrashedFolder) {
    return runTrashAction(() => commands.restoreTrashedFolder(activeLibraryId, folder.id));
  }

  function purgeTrashedFolder(folder: TrashedFolder) {
    return runTrashAction(() => commands.purgeTrashedFolder(activeLibraryId, folder.id));
  }

  function emptyTrash() {
    return runTrashAction(() => commands.emptyLibraryTrash(activeLibraryId));
  }

  async function loadDirectory(dirId: string, nextStack: FolderLevel[], generation = libraryGeneration, force = false) {
    const libraryId = activeLibraryId;
    const key = `${libraryId}:${dirId}:${hierarchyFileType}`;
    if (!libraryId || (!force && folderLoadedFor === key) || (!force && folderPendingKey === key)) return;
    const request = ++folderRequest;
    folderPendingKey = key;
    folderLoading = true;
    libraryError = "";
    try {
      const content = await measureInvoke("directory", () => commands.getLibraryFolder(libraryId, toBackendDirectoryId(dirId), hierarchyFileType));
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

  async function setAuthorSort(sort: AuthorSort) {
    if (authorSort === sort) return;
    authorSort = sort;
    focusedAuthorIndex = 0;
    await loadAuthors(libraryGeneration, true);
  }

  async function loadAuthors(generation: number, force = false) {
    const libraryId = activeLibraryId;
    if (!libraryId || authorsLoading || (!force && authorsLoadedFor === libraryId)) return;
    authorsLoading = true;
    try {
      const view = await measureInvoke("authors", () => commands.getLibraryAuthors(libraryId, null, authorSort));
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
    const key = `${libraryId}:${subjectId ?? "__root__"}:${hierarchyFileType}`;
    if (!libraryId || subjectsLoading || (!force && subjectsLoadedFor === key)) return;
    const request = ++subjectRequest;
    subjectsLoading = true;
    libraryError = "";
    try {
      const view = await measureInvoke("subjects", () => commands.getLibrarySubjects(libraryId, subjectId ?? null, hierarchyFileType));
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
    await navigate(libraryRoute("subjects", activeLibraryId, subject.id));
    await loadSubjects(subject.id, nextStack);
  }

  async function goToSubject(level: number) {
    clearLibrarySearch();
    const nextStack = subjectStack.slice(0, level);
    const subjectId = nextStack.at(-1)?.id;
    await navigate(libraryRoute("subjects", activeLibraryId, subjectId));
    await loadSubjects(subjectId, nextStack);
  }

  async function selectLibrary(libraryId: string, updateRoute = true) {
    if (activeLibraryId && activeLibraryId !== libraryId) currentSessionFromRoute();
    const generation = ++libraryGeneration;
    showLibrarySwitcher = false;
    showCreateFolder = false;
    const place = browserSession.place(libraryId);
    libraryState.select(libraryId);
    activeDestination = place.route;
    libraryError = "";
    loadingLibrary = true;
    resetLibraryData();
    folderStack = [...place.folder_path];
    subjectStack = [...place.subject_path];
    try {
      loadingLibrary = false;
      if (updateRoute) {
        const locationId = place.route === "folders" ? place.folder_path.at(-1)?.id : place.route === "subjects" ? place.subject_path.at(-1)?.id : undefined;
        await navigate(libraryRoute(place.route, libraryId, locationId));
      } else {
        await ensureDestination(place.route, generation);
      }
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
      await commitAndMeasureRender("libraries", () => libraryState.setEntries(sortedEntries), entries.length);
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
    let timeout: ReturnType<typeof setTimeout> | undefined;
    try {
      const status = await Promise.race([
        commands.getAccountStatus(),
        new Promise<never>((_, reject) => {
          timeout = setTimeout(() => reject(new Error("Session check timed out.")), ACCOUNT_STATUS_TIMEOUT_MS);
        }),
      ]);
      accountStatus = status;
      accountStatusError = "";
    } catch (reason) {
      accountStatusError = `Could not check your session: ${errorMessage(reason)}`;
    } finally {
      if (timeout) clearTimeout(timeout);
      accountResolved = true;
    }
  }

  async function refreshAccountView() {
    await loadAccountStatus();
    if (!accountStatusError && accountStatus.signed_in) {
      await loadServerTransferState().catch((reason) => (accountError = errorMessage(reason)));
    }
  }

  async function refreshAuthenticatedWebApp() {
    const enrollment = await Promise.allSettled([commands.bootstrapRemoteLibraries()]);
    const results = [...enrollment, ...await Promise.allSettled([
      loadServerTransferState(),
    ])];
    const failed = results.find((result) => result.status === "rejected");
    if (failed?.status === "rejected") libraryError = errorMessage(failed.reason);
    clearThumbnailCache();
    await loadLibraries();
  }

  async function initializeWebApp() {
    webAppReady = false;
    const session = readUiSession();
    try {
      await loadAccountStatus();
      await Promise.all([loadLibraries(), loadBrowsingPreferences(), loadReaderPreferences()]);
      await restoreUiSession(session);
    } finally {
      routesReady = true;
      webAppReady = true;
    }
    if (accountStatus.signed_in) {
      void refreshAuthenticatedWebApp().catch((reason) => (accountError = errorMessage(reason)));
    }
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
    libraryState.setEntries(sortedLibraries(catalogue.libraries));
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
    if (isWebApp) {
      accountProgress = "";
      navigate(libraryRoute("home"), true);
      void refreshAuthenticatedWebApp().catch((reason) => (accountError = errorMessage(reason)));
    } else {
      accountProgress = "Fetching libraries…";
      await loadServerTransferState();
      clearThumbnailCache();
      await loadLibraries();
      accountProgress = "";
    }
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
  const syncAccountNow = () => runAccountRequest(ACCOUNT_CONTRACT.summary.syncing_label, "", async () => {
    await commands.syncAllLibraries();
    await loadServerTransferState();
  });

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

  const registerAccount = (email: string, password: string) => runAccountRequest("Creating account…", "Check your email for the six-digit verification PIN.", () => commands.requestPublicRegistration(email, password));
  const resendAccountVerification = (email: string) => runAccountRequest("Sending verification PIN…", "If the registration is pending, a new PIN has been sent.", () => commands.resendVerification(email));
  const requestAccountPasswordReset = (email: string) => runAccountRequest("Requesting password reset…", "If the account exists, a reset token has been sent.", () => commands.requestPasswordReset(email));
  const resetAccountPassword = (token: string, password: string) => runAccountRequest("Updating password…", "Password updated. Sign in with the new password.", () => commands.resetPassword(token, password));

  async function verifyAccountEmail(email: string, pin: string): Promise<boolean> {
    if (accountBusy) return false;
    accountBusy = true;
    accountError = "";
    accountProgress = "Verifying email…";
    try {
      await finishAccountAuthentication(await commands.verifyEmail(email, pin));
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
        await loadLibraries();
        navigate(activeLibraryId ? libraryRoute("home") : "/library", true);
        webAppReady = true;
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
      libraryState.setEntries(sortedLibraries([...libraries.filter((entry) => entry.library_id !== library.library_id), library]));
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
      libraryState.setEntries(sortedLibraries(await commands.getLibraries()));
      await selectLibrary(opened.at(-1)!.library_id);
    }
    if (failures.length) libraryError = `Could not open some libraries: ${failures.join("; ")}`;
  }

  async function importWebLibraryArchive() {
    if (!isWebApp || importingLibraryArchive) return;
    libraryError = "";
    const archive = await pickLibraryArchive();
    if (!archive) return;
    importingLibraryArchive = true;
    try {
      const result = await importLibraryArchive(archive);
      libraryState.setEntries(sortedLibraries(await commands.getLibraries()));
      showLibrarySwitcher = false;
      await selectLibrary(result.library.library_id);
      if (result.failures.length) {
        libraryError = `Imported ${result.imported} book${result.imported === 1 ? "" : "s"}; ${result.failures.length} failed. ${result.failures[0]}`;
      }
      if (accountStatus.signed_in) await synchronizeBookImport();
    } catch (reason) {
      libraryError = `Could not import ZIP library: ${errorMessage(reason)}`;
    } finally {
      importingLibraryArchive = false;
    }
  }

  async function importWebFolderArchive() {
    if (!isWebApp || importingFolderArchive || !activeLibraryId) return;
    libraryError = "";
    const archive = await pickLibraryArchive();
    if (!archive) return;
    importingFolderArchive = true;
    try {
      const parentId = toBackendDirectoryId(folderStack.at(-1)?.id ?? "__root__");
      const result = await importFolderArchive(archive, activeLibraryId, parentId);
      await loadDirectory(folderStack.at(-1)?.id ?? "__root__", folderStack, libraryGeneration, true);
      if (result.failures.length) {
        libraryError = `Added “${result.folder.name}” with ${result.imported} book${result.imported === 1 ? "" : "s"}; ${result.failures.length} failed. ${result.failures[0]}`;
      }
      if (accountStatus.signed_in) await synchronizeBookImport();
    } catch (reason) {
      libraryError = `Could not add folder from ZIP: ${errorMessage(reason)}`;
    } finally {
      importingFolderArchive = false;
    }
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
      libraryState.setEntries(remaining);
      librarySyncStatuses = librarySyncStatuses.filter((status) => status.library_id !== libraryId);
      libraryTransfers = libraryTransfers.filter((group) => group.library_id !== libraryId);
      localLibraryStorageUsage = localLibraryStorageUsage.filter((usage) => usage.library_id !== libraryId);
      if (remaining.length) {
        await selectLibrary(remaining[0].library_id);
      } else {
        libraryGeneration += 1;
        libraryState.clearSelection();
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
    const nextStack = [...folderStack, { id: folder.id, name: folder.name }];
    await navigate(libraryRoute("folders", activeLibraryId, folder.id));
    await loadDirectory(folder.id, nextStack);
  }

  async function goToFolder(level: number) {
    clearLibrarySearch();
    const nextStack = folderStack.slice(0, level);
    const dirId = nextStack.at(-1)?.id ?? "__root__";
    await navigate(libraryRoute("folders", activeLibraryId, dirId));
    await loadDirectory(dirId, nextStack);
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
      const view = await measureInvoke("author_books", () => commands.getLibraryAuthors(activeLibraryId, authorId, authorSort));
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

  async function setHierarchyFileType(fileType: LibraryFileTypeFilter) {
    if (hierarchyFileType === fileType) return;
    hierarchyFileType = fileType;
    clearLibrarySearch();
    folderLoadedFor = "";
    subjectsLoadedFor = "";
    if (activeDestination === "folders") await loadDirectory(folderStack.at(-1)?.id ?? "__root__", folderStack, libraryGeneration, true);
    if (activeDestination === "subjects") await loadSubjects(subjectStack.at(-1)?.id, subjectStack, libraryGeneration, true);
  }

  async function runSearch(query: string) {
    const scope = activeDestination === "folders"
      ? `folder:${folderStack.at(-1)?.id ?? "__root__"}:${hierarchyFileType}`
      : activeDestination === "subjects"
        ? `subject:${subjectStack.at(-1)?.id ?? "__root__"}:${hierarchyFileType}`
        : "global";
    try {
      const results = await measureInvoke("search", () => scope.startsWith("folder:")
        ? commands.searchLibraryFolder(activeLibraryId, toBackendDirectoryId(folderStack.at(-1)?.id ?? "__root__"), query, BROWSING_CONTENT_CONTRACT.search_result_limit, hierarchyFileType)
        : scope.startsWith("subject:")
          ? commands.searchLibrarySubject(activeLibraryId, subjectStack.at(-1)?.id ?? null, query, BROWSING_CONTENT_CONTRACT.search_result_limit, hierarchyFileType)
          : commands.searchLibrary(activeLibraryId, query, BROWSING_CONTENT_CONTRACT.search_result_limit));
      const currentScope = activeDestination === "folders"
        ? `folder:${folderStack.at(-1)?.id ?? "__root__"}:${hierarchyFileType}`
        : activeDestination === "subjects"
          ? `subject:${subjectStack.at(-1)?.id ?? "__root__"}:${hierarchyFileType}`
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
    const local = loadWebBrowsingPreferencesFallback();
    try {
      const database = await measureInvoke("browsing_preferences", () => commands.getBrowsingPreferences());
      // localStorage is the browser's durable fallback when SQLite has to run
      // in memory, so do not replace it with that database's fresh defaults.
      browsingPreferences = local ?? database;
      saveWebBrowsingPreferencesFallback(browsingPreferences);
    } catch {
      browsingPreferences = local ?? { ...DEFAULT_BROWSING_PREFERENCES };
    }
  }

  function saveBrowsingPreference(patch: BrowsingPreferencePatch) {
    const previous = browsingPreferences;
    settingsError = "";
    const preferences = { ...browsingPreferences, [patch.preference]: patch.value } as BrowsingPreferences;
    browsingPreferences = preferences;
    saveWebBrowsingPreferencesFallback(preferences);
    const version = ++browsingPreferenceVersion;
    browsingPreferenceSaveQueue = browsingPreferenceSaveQueue.then(async () => {
      try {
        const saved = await commands.updateBrowsingPreference(patch);
        if (version === browsingPreferenceVersion) {
          browsingPreferences = saved;
          saveWebBrowsingPreferencesFallback(saved);
        }
      } catch (reason) {
        if (!isWebApp) {
          if (version === browsingPreferenceVersion) browsingPreferences = previous;
          settingsError = `Could not save browsing setting: ${errorMessage(reason)}`;
        }
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

  async function loadBookDetail(libraryId: string, contentHash: string, current: () => boolean = () => true) {
    const request = ++bookDetailRequest;
    bookDetailLoading = true;
    bookDetailError = "";
    try {
      const detail = await commands.getBookDetail(libraryId, contentHash);
      if (request === bookDetailRequest && current()) {
        bookDetail = detail;
        await tick();
        const scroll = document.querySelector<HTMLElement>(".content");
        if (scroll) scroll.scrollTop = bookDetailScrollPositions.get(`${page.url.pathname}${page.url.search}`) ?? 0;
      }
    } catch (reason) {
      if (request === bookDetailRequest && current()) {
        bookDetail = null;
        bookDetailError = errorMessage(reason);
      }
    } finally {
      // A superseding detail request owns the spinner. Otherwise always clear
      // it, including when this route synchronization became stale while the
      // backend command was in flight.
      if (request === bookDetailRequest) bookDetailLoading = false;
    }
  }

  function openBookDetailHash(contentHash: string) {
    if (!activeLibraryId) return;
    if (bookDetail) {
      const scroll = document.querySelector<HTMLElement>(".content");
      if (scroll) bookDetailScrollPositions.set(`${page.url.pathname}${page.url.search}`, scroll.scrollTop);
    }
    void navigate(`/library/${encodeURIComponent(activeLibraryId)}/book/${encodeURIComponent(contentHash)}`);
  }

  function openBookDetail(book: BookCardData) {
    openBookDetailHash(book.content_hash);
  }

  function backFromBookDetail() {
    if (history.length > 1) history.back(); else void navigate(libraryRoute(activeDestination));
  }

  function cancelBookDetailOpening() {
    bookDetailRequest += 1;
    bookDetailLoading = false;
    backFromBookDetail();
  }

  async function openDetailAuthor(id: string, name: string) {
    await navigate(libraryRoute("authors"));
    await selectAuthor(id, name);
  }

  async function openDetailSubject(path: string) {
    await navigate(libraryRoute("subjects", activeLibraryId, path));
  }

  async function openLibraryBook(book: BookCardData, initialTarget?: string) {
    openingBookId = book.content_hash;
    libraryError = "";
    try {
      const needsDownload = (bookDownloadStates[book.content_hash] ?? (book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded")) !== "Downloaded";
      if (needsDownload && !isWebApp) {
        const libraryId = activeLibraryId;
        if (!libraryId) throw new Error("No library is selected");
        const remote = await measureInvoke("open_remote_epub", () => commands.openRemoteEpub(libraryId, book.content_hash));
        if (remote) {
          pendingOpenBook = null;
          const [fileName, initialCfi] = await Promise.all([
            commands.getBookFileName(libraryId, book.content_hash),
            measureInvoke("reading_position", () => commands.getBookReadingPosition(libraryId, book.content_hash)),
          ]);
          const openedBook: ReaderBook = {
            key: libraryReaderBookKey(libraryId, book.content_hash),
            bookId: book.content_hash,
            file: { kind: "native_remote_epub", sessionId: remote.id, size: remote.size },
            fileName,
            title: book.title || fileName.replace(/\.[^.]+$/, "") || "Untitled",
            author: book.author,
            initialCfi,
            initialTarget,
            saveCfi: async (cfi, progress) => { await measureInvoke("update_reading_position", () => commands.updateReadingPosition(libraryId, book.content_hash, cfi, progress)); },
            loadAnnotations: () => commands.getAnnotations(libraryId, book.content_hash),
            upsertAnnotation: async (annotation) => { await commands.upsertAnnotation(libraryId, annotation); },
            deleteAnnotation: async (annotationId, modifiedAt) => { await commands.deleteAnnotation(libraryId, annotationId, modifiedAt); },
            loadBookmarks: () => commands.getBookmarks(libraryId, book.content_hash),
            createBookmark: async (bookmark) => { await commands.createBookmark(libraryId, bookmark); },
            deleteBookmark: async (bookmarkId, deletedAt) => { await commands.deleteBookmark(libraryId, bookmarkId, deletedAt); },
          };
          await navigateToReader(book.content_hash, openedBook);
          return;
        }
        pendingOpenBook = { book, initialTarget };
        await measureInvoke("download_book", () => downloadBook(book));
        if (bookDownloadStates[book.content_hash] !== "Downloaded") return;
      }
      pendingOpenBook = null;
      if (book.audiobook) {
        await openAudiobook(book, initialTarget);
        return;
      }
      const openedBook = await openDownloadedBook(book.content_hash, book.title || "Untitled", book.author, initialTarget);
      await navigateToReader(book.content_hash, openedBook);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      openingBookId = "";
    }
  }

  async function openAudiobook(book: BookCardData, initialTarget?: string) {
    const libraryId = activeLibraryId;
    if (!libraryId) throw new Error("No library is selected");
    if (audiobookSession?.libraryId === libraryId && audiobookSession.contentHash === book.content_hash) return;
    const request = ++audiobookRequest;
    suppressNativeAudiobookRestore = false;
    const [localPath, initialPositionMs, coverUrl, chapters] = await Promise.all([
      measureInvoke("open_audiobook", () => commands.openBook(libraryId, book.content_hash)),
      commands.getAudiobookReadingPosition(libraryId, book.content_hash),
      getThumbnailUrl(libraryId, book.content_hash),
      commands.getAudiobookChapters(libraryId, book.content_hash),
    ]);
    if (request !== audiobookRequest) return;
    const nextSession: MobileAudiobookSession = {
      key: `${libraryId}:${book.content_hash}:${request}`,
      libraryId,
      contentHash: book.content_hash,
      source: isWebApp ? localPath : convertFileSrc(localPath),
      nativeSource: localPath,
      nativeKey: `${libraryId}:${book.content_hash}`,
      title: book.title || "Untitled audiobook",
      author: book.author,
      coverUrl: coverUrl ?? "",
      initialPositionMs: parseAudiobookTarget(initialTarget) ?? initialPositionMs ?? 0,
      expectedDurationMs: book.audiobook_duration_ms,
      chapters,
      savePosition: async (positionMs, progress) => {
        await commands.updateAudiobookReadingPosition(libraryId, book.content_hash, positionMs, progress);
      },
    };
    if (audiobookSession) {
      audiobookSession = null;
      await tick();
    }
    if (request === audiobookRequest) audiobookSession = nextSession;
  }

  function parseAudiobookTarget(target?: string) {
    const prefix = "audiobook-position-ms:";
    if (!target?.startsWith(prefix)) return undefined;
    const position = Number(target.slice(prefix.length));
    return Number.isSafeInteger(position) && position >= 0 ? position : undefined;
  }

  function closeAudiobook() {
    audiobookRequest += 1;
    suppressNativeAudiobookRestore = true;
    audiobookSession = null;
  }

  async function restoreNativeAudiobook(state: NativeAudiobookState) {
    if (!state.active) {
      suppressNativeAudiobookRestore = false;
      return;
    }
    if (suppressNativeAudiobookRestore || !state.key || !state.libraryId || !state.contentHash || !state.sourcePath) return;
    if (audiobookSession?.nativeKey === state.key) return;
    const nativeKey = state.key;
    if (restoringNativeAudiobookKey === nativeKey) return;
    restoringNativeAudiobookKey = nativeKey;
    try {
      const [coverUrl, chapters] = await Promise.all([
        getThumbnailUrl(state.libraryId, state.contentHash).catch(() => null),
        commands.getAudiobookChapters(state.libraryId, state.contentHash).catch(() => []),
      ]);
      const current = window.BokheimAndroid?.getAudiobookState?.();
      if (current) {
        try {
          const latest = JSON.parse(current) as { active?: boolean; key?: string };
          if (!latest.active || latest.key !== nativeKey) return;
        } catch {
          return;
        }
      }
      const libraryId = state.libraryId;
      const contentHash = state.contentHash;
      audiobookSession = {
        key: `${nativeKey}:restored`,
        nativeKey,
        libraryId,
        contentHash,
        source: convertFileSrc(state.sourcePath),
        nativeSource: state.sourcePath,
        title: state.title || "Untitled audiobook",
        author: state.author || "",
        coverUrl: coverUrl ?? "",
        initialPositionMs: state.positionMs,
        expectedDurationMs: state.durationMs,
        chapters,
        savePosition: async (positionMs, progress) => {
          await commands.updateAudiobookReadingPosition(libraryId, contentHash, positionMs, progress);
        },
      };
    } finally {
      if (restoringNativeAudiobookKey === nativeKey) restoringNativeAudiobookKey = "";
    }
  }

  async function openDownloadedBook(bookId: string, title?: string, author?: string, initialTarget?: string): Promise<ReaderBook> {
      const libraryId = activeLibraryId;
      if (!libraryId) throw new Error("No library is selected");
      const [localPath, fileName, initialCfi, initialPdfPage] = await Promise.all([
        measureInvoke("open_book", () => commands.openBook(libraryId, bookId)),
        commands.getBookFileName(libraryId, bookId),
        measureInvoke("reading_position", () => commands.getBookReadingPosition(libraryId, bookId)),
        measureInvoke("pdf_reading_page", () => commands.getPdfReadingPage(libraryId, bookId)),
      ]);
      const pdf = /\.pdf$/i.test(fileName);
      const resolvedTitle = title || fileName.replace(/\.[^.]+$/, "") || "Untitled";
      return {
        key: libraryReaderBookKey(libraryId, bookId),
        bookId,
        file: isWebApp && localPath.startsWith("/api/")
          ? { kind: "remote_epub", url: localPath }
          : isWebApp ? localPath : convertFileSrc(localPath),
        fileName,
        title: resolvedTitle,
        author,
        initialCfi: pdf ? null : initialCfi,
        initialPdfPage: pdf ? initialPdfPage : null,
        initialTarget,
        saveCfi: async (cfi, progress) => { await measureInvoke("update_reading_position", () => commands.updateReadingPosition(libraryId, bookId, cfi, progress)); },
        savePdfPage: async (pageIndex) => { await measureInvoke("update_pdf_reading_page", () => commands.updatePdfReadingPage(libraryId, bookId, pageIndex)); },
        ...(isWebApp ? {} : {
          loadAnnotations: () => commands.getAnnotations(libraryId, bookId),
          upsertAnnotation: async (annotation) => { await commands.upsertAnnotation(libraryId, annotation); },
          deleteAnnotation: async (annotationId, modifiedAt) => { await commands.deleteAnnotation(libraryId, annotationId, modifiedAt); },
          loadBookmarks: () => commands.getBookmarks(libraryId, bookId),
          createBookmark: async (bookmark) => { await commands.createBookmark(libraryId, bookmark); },
          deleteBookmark: async (bookmarkId, deletedAt) => { await commands.deleteBookmark(libraryId, bookmarkId, deletedAt); },
        }),
      };
  }

  async function restoreReaderRoute(bookId: string, current: () => boolean = () => true) {
    const cacheKey = readerRouteCacheKey(activeLibraryId, bookId);
    const handoff = screenTransitions.handoff(cacheKey);
    if (handoff) {
      if (current()) screenTransitions.commitReader(handoff.token, cacheKey, handoff.reader);
      return;
    }
    const transition = screenTransitions.beginReaderRestore(cacheKey);
    if (!transition) return;
    try {
      const knownBook = [...recentlyAdded, ...recentlyRead, ...directoryContent.books, ...authorBooks, ...titleSearchResults, ...authorSearchResults]
        .find((book) => book.content_hash === bookId);
      const openedBook = await openDownloadedBook(bookId, knownBook?.title, knownBook?.author);
      if (!current() || !screenTransitions.commitReader(transition, cacheKey, openedBook)) return;
    } catch (reason) {
      if (!current() || !screenTransitions.isCurrent(transition)) return;
      screenTransitions.cancelReaderOpen(transition);
      libraryError = errorMessage(reason);
      await navigate(libraryRoute("home"), true);
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
      const pdf = /\.pdf$/i.test(file.name);
      const pdfStorageKey = `${storageKey}:pdf-page`;
      const storedPdfPage = Number(localStorage.getItem(pdfStorageKey));
      const openedBook: ReaderBook = {
        key: storageKey,
        file,
        title: file.name.replace(/\.(?:epub|pdf)$/i, ""),
        initialCfi: pdf ? null : localStorage.getItem(storageKey),
        initialPdfPage: pdf && Number.isInteger(storedPdfPage) && storedPdfPage >= 0 ? storedPdfPage : null,
        saveCfi: (cfi) => localStorage.setItem(storageKey, cfi),
        savePdfPage: (pageIndex) => localStorage.setItem(pdfStorageKey, String(pageIndex)),
      };
      await navigateToReader(storageKey, openedBook);
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      await picked.dispose();
    }
  }

  async function addBookToCurrentFolder() {
    if (addingBook) return;
    libraryError = "";
    if (isWebApp) await navigator.storage?.persist?.().catch(() => false);
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
      if (activeDestination === "home") await loadHome(libraryGeneration, true);
      else await loadDirectory(parentId, folderStack, libraryGeneration, true);
      await synchronizeBookImport();
    } catch (reason) {
      libraryError = errorMessage(reason);
    } finally {
      addingBook = false;
      await picked.dispose();
    }
  }

  async function synchronizeBookImport() {
    if (!accountStatus.signed_in) return;
    // An empty browser library is deliberately not created remotely. Its first
    // import makes it eligible before mutation and blob synchronization run.
    if (isWebApp) await commands.reconcileLibraryCatalogue();
    await commands.syncAllLibraries();
  }

  async function storeBookImported() {
    homeLoadedFor = "";
    authorsLoadedFor = "";
    folderLoadedFor = "";
    if (isWebApp) await navigator.storage?.persist?.().catch(() => false);
    await synchronizeBookImport();
  }

  onMount(() => {
    let disposed = false;
    let unlistenDownloadStatus: (() => void) | undefined;
    let unlistenFolderDownloadStatus: (() => void) | undefined;
    let unlistenTransferStatus: (() => void) | undefined;
    let unlistenCatalogueChanged: (() => void) | undefined;
    let transferRefreshTimer: ReturnType<typeof setTimeout> | undefined;
    let webWasBackgrounded = false;
    const unobserveNativeAudiobook = nativeAudiobookAvailable()
      ? observeNativeAudiobookState((state) => void restoreNativeAudiobook(state))
      : () => undefined;
    const unobserveNetwork = isWebApp ? () => undefined : observeNetworkStatus((status) => {
      networkStatus = status;
      void commands.updateNetworkStatus(status).then((reported) => (networkStatus = reported)).catch(() => undefined);
    });
    const unobserveApkUpdates = apkUpdaterAvailable() ? observeApkUpdates((state) => (apkUpdateState = state)) : () => undefined;
    if (apkUpdaterAvailable()) checkForApkUpdate(false);
    if (isWebApp) {
      void initializeWebApp();
    } else {
      void (async () => {
        try {
          const session = readUiSession();
          const accountStatusTask = loadAccountStatus();
          await Promise.all([
            loadLibraries(),
            loadBrowsingPreferences(),
            loadReaderPreferences(),
            loadTransferPreferences(),
          ]);
          await restoreUiSession(session);
          routesReady = true;
          await accountStatusTask;
          if (accountStatus.signed_in) {
            void loadServerTransferState().catch((reason) => {
              accountError = errorMessage(reason);
            });
          }
        } finally {
          routesReady = true;
        }
      })();
    }
    void listen<DownloadStatusEvent>("library:download-status", ({ payload }) => {
      if (payload.library_id !== activeLibraryId) return;
      const change = payload.event.DownloadStatusChanged;
      bookDownloadStates = { ...bookDownloadStates, [change.content_hash]: change.state };
      if (pendingOpenBook?.book.content_hash === change.content_hash && change.state === "Downloaded" && openingBookId !== change.content_hash) {
        const pending = pendingOpenBook;
        pendingOpenBook = null;
        queueMicrotask(() => void openLibraryBook(pending.book, pending.initialTarget));
      } else if (pendingOpenBook?.book.content_hash === change.content_hash && change.state === "NotDownloaded") {
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
      const destination = [...NAV_ITEMS, ...APP_NAV_ITEMS][Number(event.key) - 1]?.id;
      if (destination) {
        event.preventDefault();
        setDestination(destination);
      } else if (event.key.toLocaleLowerCase() === "f") {
        event.preventDefault();
        focusContextualSearch();
      }
    };
    const handleVisibilityChange = () => {
      if (document.visibilityState === "hidden") {
        webWasBackgrounded = true;
      } else if (webWasBackgrounded) {
        webWasBackgrounded = false;
        void synchronizeForegroundWebApp(true);
      }
    };
    const handleWindowFocus = () => void synchronizeForegroundWebApp();
    document.addEventListener("keydown", handleShortcut);
    if (isWebApp) {
      document.addEventListener("visibilitychange", handleVisibilityChange);
      window.addEventListener("focus", handleWindowFocus);
    }
    return () => {
      disposed = true;
      unlistenDownloadStatus?.();
      unlistenFolderDownloadStatus?.();
      unlistenTransferStatus?.();
      unlistenCatalogueChanged?.();
      if (transferRefreshTimer) clearTimeout(transferRefreshTimer);
      document.removeEventListener("keydown", handleShortcut);
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      window.removeEventListener("focus", handleWindowFocus);
      unobserveNetwork();
      unobserveNativeAudiobook();
      unobserveApkUpdates();
      if (searchTimer) clearTimeout(searchTimer);
      clearThumbnailCache();
    };
  });
</script>

<svelte:head><title>{readerBook ? `${readerBook.title} · Reader` : `${destinationTitle} · ${APP_NAME}`}</title></svelte:head>

{#if isWebApp && (!accountResolved || !webAppReady)}
  <main class="auth-loading" aria-live="polite">
    <span class="loader"></span>
    <strong>{accountResolved ? "Opening your library" : "Starting the local reader"}</strong>
  </main>
{:else if applicationSurface.phase === "opening-reader" || applicationSurface.phase === "closing-reader"}
  <main class="screen-transition" aria-live="polite">
    <span class="loader"></span>
    <strong>{applicationSurface.phase === "closing-reader" ? "Returning to library" : "Opening reader"}</strong>
  </main>
{:else if readerBook}
  {#key readerBook.key}
    {#if isPdfReaderBook(readerBook)}
      <PdfReader book={readerBook} onclose={closeReader} />
    {:else}
      <Reader
        book={readerBook}
        preferences={readerPreferences}
        applicationTheme={browsingPreferences.theme}
        applicationAppearance={resolveAppearance(browsingPreferences.appearance)}
        onpreferenceschange={saveReaderPreference}
        onclose={closeReader}
      />
    {/if}
  {/key}
{:else}
  <BrowserRoot
    active={activeDestination} pageTitle={destinationTitle} libraryName={activeLibrary?.library_name ?? "Libraries"}
    {libraries} {activeLibraryId} breadcrumbs={activeDestination === "subjects" ? subjectStack : folderStack}
    style={browsingLayoutStyle} detailMode={bookDetailLoading || bookDetail !== null || Boolean(bookDetailError)} playerOpen={audiobookSession !== null} {addingBook} {importingFolderArchive}
    {showLibrarySwitcher} {showCreateFolder} {showCreateLibrary} {creatingLibrary} {importingLibraryArchive}
    onnavigate={setDestination} onlibraries={openLibrarySwitcher} onaccount={() => setDestination("account")} onsettings={() => setDestination("settings")}
    onaddbook={addBookToCurrentFolder} onnewfolder={() => (showCreateFolder = true)} onimportfolder={importWebFolderArchive} onbreadcrumb={activeDestination === "subjects" ? goToSubject : goToFolder}
    onselectlibrary={selectLibrary} oncreatelibrary={() => { showLibrarySwitcher = false; showCreateLibrary = true; }} onopenlibrary={openLibraryFolders} onimportlibraryarchive={importWebLibraryArchive}
    onremovelibrary={removeLibraryFromDevice} oncloselibraryswitcher={() => (showLibrarySwitcher = false)}
    onclosecreatefolder={() => (showCreateFolder = false)} oncreatefolder={createFolder}
    onclosecreatelibrary={() => (showCreateLibrary = false)} oncreate={createLibrary}
  >
    {#snippet content()}
      {#if bookDetailLoading}
        <section class="detail-state" aria-live="polite"><span class="loader"></span><strong>Opening book details</strong><button type="button" onclick={cancelBookDetailOpening}>Back</button></section>
      {:else if bookDetailError}
        <section class="detail-state" role="alert"><strong>Book unavailable</strong><p>{bookDetailError}</p><button type="button" onclick={backFromBookDetail}>Back</button></section>
      {:else if bookDetail}
        <BookDetailPage detail={bookDetail} libraryId={activeLibraryId} {openingBookId} downloadStates={bookDownloadStates} onback={backFromBookDetail} onread={openLibraryBook} onrelated={openBookDetail} onauthor={openDetailAuthor} onsubject={openDetailSubject} ondownload={downloadBook} onremove={removeLocalBookCopy} ontrash={trashDetailBook} oncopy={copyDetailBookToFolder} onmove={moveDetailBookToFolder} />
      {:else if activeDestination === "account" || activeDestination === "settings" || activeDestination === "store"}
        <UniversalHost destination={activeDestination} {activeLibraryId} libraryName={activeLibrary?.library_name ?? "library"} {accountResolved} {accountStatusError} account={accountStatus} {accountBusy} {accountProgress} {accountError} {accountStorageUsage} {librarySyncStatuses} {libraryTransfers} {localLibraryStorageUsage} {serverLibraryStorageUsage} preferences={browsingPreferences} {networkStatus} {transferPreferences} {settingsError} {isWebApp} onrefresh={syncAccountNow} onretry={refreshAccountView} onlogin={loginToServer} onlogout={logoutFromServer} onregister={registerAccount} onresendverification={resendAccountVerification} onverifyemail={verifyAccountEmail} onrequestpasswordreset={requestAccountPasswordReset} onresetpassword={resetAccountPassword} onpreferencechange={saveBrowsingPreference} ontransferpreference={saveTransferPreference} onimported={storeBookImported} oncreatelibrary={() => setDestination("folders")} />
      {:else}
        <LibraryHost destination={activeDestination} configured={libraries.length > 0} loading={loadingLibrary} libraryId={activeLibraryId} preferences={browsingPreferences} inProgress={visibleRecentlyInProgress} finished={visibleRecentlyFinished} recentlyAdded={visibleRecentlyAdded} {directoryContent} folderPath={folderStack} {subjectContent} subjectPath={subjectStack} fileType={hierarchyFileType} {subjectsLoading} authors={visibleAuthors} bind:authorFilter {authorSort} bind:focusedAuthorIndex {selectedAuthorId} {selectedAuthorName} authorBooks={visibleAuthorBooks} bind:searchQuery {searchPending} {searchCompletedQuery} titleSearchResults={visibleTitleSearchResults} authorSearchResults={visibleAuthorSearchResults} {trash} {trashLoading} {trashBusy} {trashError} {openingBookId} downloadStates={bookDownloadStates} {folderDownloadStatuses} {activeFolderDownloads} error={libraryError} oncreateLibrary={() => (showCreateLibrary = true)} onadd={addBookToCurrentFolder} onopen={openBookDetail} onopenbookhash={openBookDetailHash} ondownload={downloadBook} onremove={removeLocalBookCopy} ontrashbook={moveBookToTrash} oncopybook={copyBookToFolder} onmovebook={moveBookToFolder} onremovebookfromfolder={removeBookFromFolder} onopenfolder={openFolder} ondownloadfolder={downloadFolder} ontrashfolder={moveFolderToTrash} onrenamefolder={renameFolder} onmovefolder={chooseFolderMove} onnewfolder={() => (showCreateFolder = true)} onsearch={scheduleSearch} onfiletype={setHierarchyFileType} onopensubject={openSubject} onfilterkeydown={handleAuthorFilterKeydown} onsort={setAuthorSort} onauthorkeydown={handleAuthorKeydown} onselectauthor={selectAuthor} onclearauthor={clearSelectedAuthor} onimported={storeBookImported} onretry={retryActiveSection} onloadtrash={loadTrash} onrestorebook={restoreTrashedBook} onpurgebook={purgeTrashedBook} onrestorefolder={restoreTrashedFolder} onpurgefolder={purgeTrashedFolder} onemptytrash={emptyTrash} />
      {/if}
    {/snippet}
  </BrowserRoot>
  {#if folderPickerOperation}
    <FolderPickerDialog
      title={folderPickerTitle(folderPickerOperation)}
      actionLabel={folderPickerOperation.kind === "copy_book" ? "Copy" : "Move"}
      destinations={folderPickerDestinations}
      initialId={folderPickerDestinations[0]?.id ?? ""}
      onclose={closeFolderPicker}
      onconfirm={confirmFolderPicker}
    />
  {/if}
{/if}

{#if showApkUpdateNotice}
  <UpdateNotice state={apkUpdateState} oninstall={() => apkUpdateState.phase === "permission_required" ? openApkInstallSettings() : installApkUpdate(apkUpdateState)} ondismiss={() => (dismissedUpdateVersion = apkUpdateState.version ?? "dismissed")} />
{/if}

{#if audiobookSession}
  <AudiobookPlayer session={audiobookSession} aboveNavigation={!readerBook && applicationSurface.phase === "browser"} onclose={closeAudiobook} />
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
  .auth-loading { display: grid; min-height: 100dvh; place-content: center; justify-items: center; gap: .8rem; color: var(--color-subtle-text); background: var(--color-surface-bg); font-size: .8rem; }
  .screen-transition { display: grid; min-height: 100dvh; place-content: center; justify-items: center; gap: .8rem; color: var(--color-subtle-text); background: var(--color-surface-bg); font-size: .8rem; pointer-events: none; }
  .loader { width: 1.4rem; height: 1.4rem; border: .18rem solid var(--color-border); border-top-color: var(--color-accent); border-radius: 50%; animation: spin .8s linear infinite; }
  .detail-state { display:grid; min-height:18rem; place-content:center; justify-items:center; gap:.75rem; color:var(--color-subtle-text); text-align:center; } .detail-state p { max-width:42ch; margin:0; } .detail-state button { min-height:2.75rem; padding:.55rem 1rem; border:1px solid var(--color-border); border-radius: 0; color:var(--color-ink-text); background:var(--color-raised-bg); cursor:pointer; font:inherit; }
  @keyframes spin { to { transform: rotate(360deg); } }

  @media (prefers-reduced-motion: reduce) {
    .loader { animation: none; }
  }

</style>
