import { writable } from "svelte/store";
import * as bookActions from "../../../../../lib/library/book-actions";
import { commands } from "../../../../../lib/app/commands";
import { createScopedSearch } from "../../../../../lib/library/scoped-search";
import { libraryBookHref, libraryFolderHref } from "../../../../../lib/app/navigation";
import { isWebApp } from "../../../../../lib/app/platform";
import { toBackendDirectoryId } from "../../../../../lib/library/directory-id";
import { pickEpub } from "../../../../../lib/library/epub-picker";
import { synchronizeBookImport } from "../../../../../lib/library/import-sync";
import { requestLibraryRefresh } from "../../../../../lib/app/library-events";
import type { FolderPickerOperation } from "../../../../../lib/library/folder-types";
import type { LibraryPageLevel } from "../../../../../lib/library/page-types";
import {
  BROWSING_CONTENT_CONTRACT,
  type BookCardData,
  type DirectoryContent,
  type DownloadState,
  type FolderDownloadStatus,
  type LibraryDirectory,
  type LibraryFileTypeFilter,
} from "../../../../../lib/shared/types";
import type { LibraryFolderDestination } from "../../../../../lib/bindings";

const EMPTY: DirectoryContent = { children: [], books: [] };
const message = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);
const bookStates = (books: BookCardData[]): Record<string, DownloadState> =>
  Object.fromEntries(
    books.map((book) => [
      book.content_hash,
      book.downloaded
        ? "Downloaded"
        : book.download_requested
          ? "Queued"
          : "NotDownloaded",
    ]),
  );

export interface FolderRouteState {
  libraryId: string;
  folderId: string;
  loading: boolean;
  error: string;
  content: DirectoryContent;
  path: LibraryPageLevel[];
  fileType: LibraryFileTypeFilter;
  query: string;
  searchPending: boolean;
  searchCompletedQuery: string;
  titleSearchResults: BookCardData[];
  authorSearchResults: BookCardData[];
  detailed: boolean;
  downloadStates: Record<string, DownloadState>;
  folderDownloadStatuses: Record<string, FolderDownloadStatus>;
  activeFolderDownloads: Record<string, boolean>;
  addingBook: boolean;
  showCreateFolder: boolean;
  importingFolder: boolean;
  pickerOperation: FolderPickerOperation | null;
  pickerDestinations: LibraryFolderDestination[];
}
const initial = (): FolderRouteState => ({
  libraryId: "",
  folderId: "__root__",
  loading: false,
  error: "",
  content: EMPTY,
  path: [],
  fileType: "all",
  query: "",
  searchPending: false,
  searchCompletedQuery: "",
  titleSearchResults: [],
  authorSearchResults: [],
  detailed: false,
  downloadStates: {},
  folderDownloadStatuses: {},
  activeFolderDownloads: {},
  addingBook: false,
  showCreateFolder: false,
  importingFolder: false,
  pickerOperation: null,
  pickerDestinations: [],
});

export function createFolderRoute(navigate: (path: string) => Promise<void>) {
  const store = writable<FolderRouteState>(initial());
  const { subscribe } = store;
  let state = initial();
  let libraryId = "";
  let request = 0;
  const update = (fn: (current: FolderRouteState) => FolderRouteState) => {
    state = fn(state);
    store.set(state);
  };
  const folderStatus = (folders: LibraryDirectory[]) =>
    Object.fromEntries(
      folders
        .filter((folder) => folder.id !== "__root__")
        .map((folder) => [
          folder.id,
          {
            dir_id: folder.id,
            downloaded_books: folder.downloaded_book_count,
            total_books: folder.book_count,
          },
        ]),
    );
  const parentId = () => toBackendDirectoryId(state.folderId);
  async function load(nextLibraryId: string, folderId = "__root__", force = false) {
    if (
      !nextLibraryId ||
      (!force && libraryId === nextLibraryId && state.folderId === folderId)
    )
      return;
    libraryId = nextLibraryId;
    const current = ++request;
    // The query belongs to the folder being left, not the one being opened.
    search.cancel();
    update((s) => ({
      ...s,
      libraryId: nextLibraryId,
      folderId,
      loading: true,
      error: "",
      query: "",
      searchPending: false,
      searchCompletedQuery: "",
      titleSearchResults: [],
      authorSearchResults: [],
    }));
    try {
      const [content, path, preferences] = await Promise.all([
        commands.getLibraryFolder(
          nextLibraryId,
          toBackendDirectoryId(folderId),
          state.fileType,
        ),
        folderId === "__root__"
          ? Promise.resolve([])
          : commands.getLibraryFolderPath(nextLibraryId, toBackendDirectoryId(folderId)),
        commands.getBrowsingPreferences(),
      ]);
      if (current !== request) return;
      update((s) => ({
        ...s,
        loading: false,
        content,
        path,
        detailed: preferences.library_view === "detailed",
        downloadStates: bookStates(content.books),
        folderDownloadStatuses: folderStatus(content.children),
      }));
    } catch (reason) {
      if (current === request)
        update((s) => ({ ...s, loading: false, error: message(reason) }));
    }
  }
  function setQuery(query: string) {
    update((s) => ({ ...s, query }));
  }

  const search = createScopedSearch(
    (query) =>
      commands.searchLibraryFolder(
        libraryId,
        parentId(),
        query,
        BROWSING_CONTENT_CONTRACT.search_result_limit,
        state.fileType,
      ),
    {
      cleared: () =>
        update((s) => ({
          ...s,
          searchPending: false,
          searchCompletedQuery: "",
          titleSearchResults: [],
          authorSearchResults: [],
        })),
      pending: () => update((s) => ({ ...s, searchPending: true })),
      results: (query, results) =>
        update((s) => ({
          ...s,
          searchPending: false,
          searchCompletedQuery: query,
          titleSearchResults: results.title_matches,
          authorSearchResults: results.author_matches,
          downloadStates: {
            ...s.downloadStates,
            ...bookStates([...results.title_matches, ...results.author_matches]),
          },
        })),
      failed: (reason) =>
        update((s) => ({ ...s, searchPending: false, error: message(reason) })),
    },
  );
  const scheduleSearch = () => search.schedule(state.query);
  async function refresh() {
    await load(libraryId, state.folderId, true);
  }

  /**
   * Folder mutations change the hierarchy every page reads, not just this one:
   * a trashed or renamed folder shows up in Trash, on the home page's counts
   * and in any folder picker.
   */
  async function refreshEverywhere() {
    await refresh();
    requestLibraryRefresh();
  }
  async function setFileType(fileType: LibraryFileTypeFilter) {
    if (fileType === state.fileType) return;
    update((s) => ({ ...s, fileType }));
    await refresh();
  }
  function setDownloadState(contentHash: string, value: DownloadState) {
    update((s) => ({
      ...s,
      downloadStates: { ...s.downloadStates, [contentHash]: value },
    }));
  }
  function setFolderDownloadStatus(status: FolderDownloadStatus) {
    update((s) => ({
      ...s,
      folderDownloadStatuses: { ...s.folderDownloadStatuses, [status.dir_id]: status },
    }));
  }
  async function downloadFolder(folder: LibraryDirectory) {
    if (state.activeFolderDownloads[folder.id]) return;
    update((s) => ({
      ...s,
      activeFolderDownloads: { ...s.activeFolderDownloads, [folder.id]: true },
    }));
    try {
      const value = await commands.downloadFolder(
        libraryId,
        toBackendDirectoryId(folder.id),
      );
      update((s) => ({
        ...s,
        folderDownloadStatuses: { ...s.folderDownloadStatuses, [folder.id]: value },
      }));
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    } finally {
      update((s) => ({
        ...s,
        activeFolderDownloads: { ...s.activeFolderDownloads, [folder.id]: false },
      }));
    }
  }
  async function create(name: string) {
    await commands.createDirectory(libraryId, parentId(), name);
    await refreshEverywhere();
  }
  async function rename(folder: LibraryDirectory) {
    const name = window.prompt(`Rename “${folder.name}”`, folder.name)?.trim();
    if (!name || name === folder.name) return;
    try {
      await commands.renameFolder(libraryId, toBackendDirectoryId(folder.id), name);
      await refreshEverywhere();
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    }
  }
  async function trashFolder(folder: LibraryDirectory) {
    if (!window.confirm(`Move the folder “${folder.name}” and its contents to Trash?`))
      return;
    try {
      await commands.moveFolderToTrash(libraryId, toBackendDirectoryId(folder.id));
      await refreshEverywhere();
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    }
  }
  async function openPicker(operation: FolderPickerOperation) {
    try {
      const destinations =
        operation.kind === "move_folder"
          ? await commands.getFolderDestinations(
              libraryId,
              operation.folder.id,
              operation.sourceParentId,
            )
          : await commands.getFolderDestinations(libraryId, operation.sourceId, null);
      if (!destinations.length) throw new Error("There are no valid destination folders");
      update((s) => ({
        ...s,
        pickerOperation: operation,
        pickerDestinations: destinations,
      }));
    } catch (reason) {
      update((s) => ({
        ...s,
        error: `Could not open folder picker: ${message(reason)}`,
      }));
    }
  }
  async function confirmPicker(destinationId: string) {
    const operation = state.pickerOperation;
    if (!operation) return;
    if (operation.kind === "copy_book")
      await commands.copyBookToFolder(
        libraryId,
        operation.book.content_hash,
        operation.sourceId,
        destinationId,
      );
    else if (operation.kind === "move_book")
      await commands.moveBookToFolder(
        libraryId,
        operation.book.content_hash,
        operation.sourceId,
        destinationId,
      );
    else
      await commands.moveFolder(
        libraryId,
        toBackendDirectoryId(operation.folder.id),
        destinationId,
      );
    update((s) => ({ ...s, pickerOperation: null, pickerDestinations: [] }));
    await refreshEverywhere();
  }
  async function removeFromFolder(book: BookCardData) {
    if (
      !window.confirm(
        `Remove “${book.title || "Untitled"}” from this folder? If it has no other placement, it will move to Trash.`,
      )
    )
      return;
    try {
      await commands.removeBookFromFolder(libraryId, book.content_hash, parentId());
      await refreshEverywhere();
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    }
  }
  async function add() {
    if (state.addingBook) return;
    let picked;
    try {
      picked = await pickEpub(true);
      if (!picked) return;
      update((s) => ({ ...s, addingBook: true }));
      const source = await picked.asImportPath();
      if (!source) throw new Error("Adding books requires the native application.");
      if (isWebApp) await navigator.storage?.persist?.();
      await commands.importBook(
        { library_id: libraryId, directory_id: parentId() },
        source,
        picked.name,
      );
      await refreshEverywhere();
      await synchronizeBookImport();
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    } finally {
      update((s) => ({ ...s, addingBook: false }));
      await picked?.dispose();
    }
  }
  async function importFolder() {
    if (!isWebApp || state.importingFolder) return;
    try {
      const { importFolderArchive, pickLibraryArchive } =
        await import("../../../../../lib/web/library-archive");
      const archive = await pickLibraryArchive();
      if (!archive) return;
      update((s) => ({ ...s, importingFolder: true }));
      const result = await importFolderArchive(archive, libraryId, parentId());
      await refreshEverywhere();
      await synchronizeBookImport();
      if (result.failures.length)
        update((s) => ({
          ...s,
          error: `Added “${result.folder.name}” with ${result.imported} book${result.imported === 1 ? "" : "s"}; ${result.failures.length} failed. ${result.failures[0]}`,
        }));
    } catch (reason) {
      update((s) => ({
        ...s,
        error: `Could not add folder from ZIP: ${message(reason)}`,
      }));
    } finally {
      update((s) => ({ ...s, importingFolder: false }));
    }
  }
  async function download(book: BookCardData) {
    try {
      await bookActions.downloadBook(libraryId, book);
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    }
  }
  async function remove(book: BookCardData) {
    try {
      await bookActions.removeLocalCopy(libraryId, book);
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    }
  }
  async function trash(book: BookCardData) {
    try {
      if (await bookActions.trashBook(libraryId, book)) {
      }
    } catch (reason) {
      update((s) => ({ ...s, error: message(reason) }));
    }
  }

  return {
    subscribe,
    load,
    setDownloadState,
    setFolderDownloadStatus,
    refresh,
    setQuery,
    scheduleSearch,
    setFileType,
    open: (book: BookCardData) => navigate(libraryBookHref(libraryId, book.content_hash)),
    openFolder: (folder: LibraryDirectory) =>
      navigate(libraryFolderHref(libraryId, folder.id)),
    goToFolder: (level: number) =>
      navigate(libraryFolderHref(libraryId, state.path.slice(0, level).at(-1)?.id)),
    download,
    remove,
    trash,
    downloadFolder,
    create,
    rename,
    trashFolder,
    copy: (book: BookCardData) =>
      openPicker({ kind: "copy_book", book, sourceId: parentId() }),
    move: (book: BookCardData) =>
      openPicker({ kind: "move_book", book, sourceId: parentId() }),
    moveFolder: (folder: LibraryDirectory) =>
      openPicker({ kind: "move_folder", folder, sourceParentId: parentId() }),
    confirmPicker,
    closePicker: () =>
      update((s) => ({ ...s, pickerOperation: null, pickerDestinations: [] })),
    removeFromFolder,
    add,
    importFolder,
    showCreateFolder: () => update((s) => ({ ...s, showCreateFolder: true })),
    closeCreateFolder: () => update((s) => ({ ...s, showCreateFolder: false })),
  };
}
