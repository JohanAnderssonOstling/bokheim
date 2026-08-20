/// <reference lib="webworker" />

import type {
  AuthorSort,
  BookImportDestination,
  BrowsingPreferencePatch,
  CatalogImportRequest,
  CatalogSearch,
  CatalogSearchQuery,
  LibraryFileTypeFilter,
  ReaderPreferencePatch,
} from "../lib/bindings";
import {
  getWebAccountStatus,
  getWebAccountStorageUsage,
  getWebServerLibraryStorageUsage,
  loginWebAccount,
  logoutWebAccount,
  registerWebAccount,
  requestWebPasswordReset,
  resendWebVerification,
  resetWebPassword,
  verifyWebEmail,
} from "../lib/web/account-client";
import {
  bootstrapWebRemoteLibraries,
  reconcileWebLibraries,
} from "../lib/web/library-list-sync";
import { synchronizeAllWebLibraries } from "../lib/web/library-sync";
import { getWebLocalStorageUsage, getWebTransferStatuses } from "../lib/web/blob-sync";
import { getWebDatabaseHealth, persistWebDatabaseFallback } from "../lib/web/database";
import { selectFolderDestinations } from "../lib/web/wasm-core";
import {
  getWebCatalogCover,
  getWebCatalogPage,
  getWebCatalogSubjects,
  getWebCatalogSources,
  discoverWebCatalogAuthorBooks,
  importWebCatalogBook,
  refreshWebCatalogSources,
  searchWebCatalog,
  searchWebCatalogs,
  getWebCatalogSubjectRecommendations,
} from "../lib/web/catalog-client";
import {
  createWebLibrary,
  createWebDirectory,
  getWebFolderDestinations,
  renameWebFolder,
  moveWebFolder,
  copyWebBookToFolder,
  moveWebBookToFolder,
  removeWebBookFromFolder,
  getWebChildDirectories,
  getWebDirectoryPath,
  getWebBookFileName,
  getWebBookThumbnail,
  getWebBookDetail,
  getWebLibraryHierarchy,
  getWebLibraryAuthors,
  getWebLibraryFolder,
  getWebLibraryHome,
  getWebLibrarySubjects,
  getWebLibraryTrash,
  moveWebBookToTrash,
  moveWebFolderToTrash,
  getWebReadingPosition,
  getWebPdfReadingPosition,
  getWebAudiobookChapters,
  importWebBook,
  listWebLibraries,
  openWebBook,
  removeWebLibrary,
  restoreWebTrashedBook,
  restoreWebTrashedFolder,
  purgeWebTrashedBook,
  purgeWebTrashedFolder,
  emptyWebLibraryTrash,
  searchWebLibrary,
  searchWebLibrarySubject,
  updateWebReadingPosition,
  updateWebPdfReadingPosition,
} from "../lib/web/library-database";

let mutationSyncTimer: ReturnType<typeof setTimeout> | undefined;
let mutationSyncRetry = 0;
function scheduleMutationSync(): void {
  if (mutationSyncTimer) clearTimeout(mutationSyncTimer);
  mutationSyncTimer = setTimeout(
    () => {
      mutationSyncTimer = undefined;
      void getWebAccountStatus()
        .then(async (account) => {
          if (!account.signed_in) return;
          await reconcileWebLibraries();
          await synchronizeAllWebLibraries();
          await persistWebDatabaseFallback();
        })
        .then(() => {
          mutationSyncRetry = 0;
        })
        .catch(() => {
          mutationSyncRetry = Math.min(mutationSyncRetry + 1, 6);
          mutationSyncTimer = setTimeout(
            scheduleMutationSync,
            Math.min(60_000, 1000 * 2 ** mutationSyncRetry),
          );
        });
    },
    mutationSyncRetry ? 0 : 400,
  );
}
import type {
  WorkerCommandRequest,
  WorkerCommandResponse,
} from "../lib/web/worker-protocol";
import {
  getWebBrowsingPreferences,
  getWebReaderPreferences,
  updateWebBrowsingPreference,
  updateWebReaderPreference,
} from "../lib/web/preferences-database";

function stringArg(request: WorkerCommandRequest, index: number, name: string): string {
  const value = request.args[index];
  if (typeof value !== "string") throw new Error(`${name} must be a string`);
  return value;
}

function nullableStringArg(
  request: WorkerCommandRequest,
  index: number,
  name: string,
): string | null {
  const value = request.args[index];
  if (value === null) return null;
  if (typeof value !== "string") throw new Error(`${name} must be a string or null`);
  return value;
}

function stringArrayArg(
  request: WorkerCommandRequest,
  index: number,
  name: string,
): string[] {
  const value = request.args[index];
  if (!Array.isArray(value) || !value.every((item) => typeof item === "string"))
    throw new Error(`${name} must be an array of strings`);
  return value;
}

function fileTypeArg(
  request: WorkerCommandRequest,
  index: number,
): LibraryFileTypeFilter {
  const value = request.args[index] ?? "all";
  if (value === "all" || value === "book" || value === "pdf" || value === "audiobook")
    return value;
  throw new Error("file_type is invalid");
}

function nullableNumberArg(
  request: WorkerCommandRequest,
  index: number,
  name: string,
): number | null {
  const value = request.args[index];
  if (value === null) return null;
  if (typeof value !== "number" || !Number.isFinite(value))
    throw new Error(`${name} must be a finite number or null`);
  return value;
}

function numberArg(request: WorkerCommandRequest, index: number, name: string): number {
  const value = nullableNumberArg(request, index, name);
  if (value === null) throw new Error(`${name} must be a number`);
  return value;
}

function objectArg<T extends object>(
  request: WorkerCommandRequest,
  index: number,
  name: string,
): T {
  const value = request.args[index];
  if (!value || typeof value !== "object") throw new Error(`${name} is invalid`);
  return value as T;
}

async function executeCommand(request: WorkerCommandRequest): Promise<unknown> {
  switch (request.command) {
    case "init_logging":
      return null;
    case "backend_link_status": {
      const health = await getWebDatabaseHealth();
      return `web-backend-worker-ready sqlite-${health.sqlite_version} schema-${health.schema_version} ${health.persistence}`;
    }
    case "get_account_status":
      await getWebDatabaseHealth();
      return getWebAccountStatus();
    case "login":
      return loginWebAccount(
        stringArg(request, 0, "email"),
        stringArg(request, 1, "password"),
      );
    case "request_public_registration":
      return registerWebAccount(
        stringArg(request, 0, "email"),
        stringArg(request, 1, "password"),
      );
    case "resend_verification":
      return resendWebVerification(stringArg(request, 0, "email"));
    case "verify_email":
      return verifyWebEmail(stringArg(request, 0, "email"), stringArg(request, 1, "pin"));
    case "request_password_reset":
      return requestWebPasswordReset(stringArg(request, 0, "email"));
    case "reset_password":
      return resetWebPassword(
        stringArg(request, 0, "token"),
        stringArg(request, 1, "new_password"),
      );
    case "logout":
      return logoutWebAccount();
    case "bootstrap_remote_libraries":
      await bootstrapWebRemoteLibraries();
      await synchronizeAllWebLibraries();
      return listWebLibraries();
    case "reconcile_libraries":
      return reconcileWebLibraries();
    case "sync_all_libraries":
      await reconcileWebLibraries();
      return synchronizeAllWebLibraries();
    case "get_transfer_statuses":
      return getWebTransferStatuses();
    case "get_local_library_storage_usage":
      return getWebLocalStorageUsage();
    case "get_account_storage_usage":
      return getWebAccountStorageUsage();
    case "get_server_library_storage_usage":
      return getWebServerLibraryStorageUsage();
    case "get_reader_preferences":
      return getWebReaderPreferences();
    case "update_reader_preference":
      return updateWebReaderPreference(
        objectArg<ReaderPreferencePatch>(request, 0, "patch"),
      );
    case "get_browsing_preferences":
      return getWebBrowsingPreferences();
    case "update_browsing_preference":
      return updateWebBrowsingPreference(
        objectArg<BrowsingPreferencePatch>(request, 0, "patch"),
      );
    case "get_libraries":
      return listWebLibraries();
    case "get_catalog_sources":
      return getWebCatalogSources();
    case "refresh_catalog_sources":
      return refreshWebCatalogSources();
    case "get_custom_catalog_sources":
      return [];
    case "get_catalog_page":
      return getWebCatalogPage(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "source_id"),
        nullableStringArg(request, 2, "locator"),
      );
    case "get_catalog_subjects":
      return getWebCatalogSubjects(
        stringArg(request, 0, "library_id"),
        nullableStringArg(request, 1, "locator"),
        stringArrayArg(request, 2, "source_ids"),
        stringArg(request, 3, "query"),
        numberArg(request, 4, "limit"),
      );
    case "get_catalog_subject_recommendations":
      return getWebCatalogSubjectRecommendations(
        stringArg(request, 0, "library_id"),
        nullableStringArg(request, 1, "subject_path"),
        numberArg(request, 2, "limit"),
      );
    case "search_catalogs":
      return searchWebCatalogs(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "query"),
      );
    case "search_catalog":
      return searchWebCatalog(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "source_id"),
        objectArg<CatalogSearch>(request, 2, "search"),
        objectArg<CatalogSearchQuery>(request, 3, "query"),
      );
    case "get_catalog_cover":
      return getWebCatalogCover(
        stringArg(request, 0, "source_id"),
        stringArg(request, 1, "image_url"),
      );
    case "import_catalog_book":
      await importWebCatalogBook(
        objectArg<BookImportDestination>(request, 0, "destination"),
        objectArg<CatalogImportRequest>(request, 1, "request"),
      );
      scheduleMutationSync();
      return null;
    case "discover_catalog_author_books":
      return discoverWebCatalogAuthorBooks(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "author_id"),
      );
    case "create_library":
      return createWebLibrary(stringArg(request, 0, "name"));
    case "remove_library":
      await removeWebLibrary(stringArg(request, 0, "library_id"));
      void reconcileWebLibraries().catch(() => undefined);
      return null;
    case "get_library_home":
      return getWebLibraryHome(stringArg(request, 0, "library_id"));
    case "get_book_detail":
      return getWebBookDetail(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "get_library_folder":
      return getWebLibraryFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
        fileTypeArg(request, 2),
      );
    case "get_library_subjects":
      return getWebLibrarySubjects(
        stringArg(request, 0, "library_id"),
        nullableStringArg(request, 1, "subject_path"),
        fileTypeArg(request, 2),
      );
    case "get_edition_identity_reviews":
      return [];
    case "select_edition_identity_candidate":
    case "apply_manual_edition_identity":
    case "reject_edition_identity_candidates":
    case "retry_edition_identity":
      throw new Error("ISBN metadata review is available in the installed Bokheim app");
    case "get_library_trash":
      return getWebLibraryTrash(stringArg(request, 0, "library_id"));
    case "move_book_to_trash":
      await moveWebBookToTrash(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
      scheduleMutationSync();
      return null;
    case "move_folder_to_trash":
      await moveWebFolderToTrash(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
      );
      scheduleMutationSync();
      return null;
    case "restore_trashed_book":
      await restoreWebTrashedBook(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
      scheduleMutationSync();
      return null;
    case "purge_trashed_book":
      await purgeWebTrashedBook(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
      scheduleMutationSync();
      return null;
    case "restore_trashed_folder": {
      const name = await restoreWebTrashedFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
      );
      scheduleMutationSync();
      return name;
    }
    case "purge_trashed_folder":
      await purgeWebTrashedFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
      );
      scheduleMutationSync();
      return null;
    case "empty_library_trash":
      await emptyWebLibraryTrash(stringArg(request, 0, "library_id"));
      scheduleMutationSync();
      return null;
    case "get_child_directories":
      return getWebChildDirectories(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "parent_id"),
      );
    case "get_library_folder_path":
      return getWebDirectoryPath(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
      );
    case "create_directory": {
      const directory = await createWebDirectory(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "parent_id"),
        stringArg(request, 2, "name"),
      );
      scheduleMutationSync();
      return directory;
    }
    case "get_folder_destinations": {
      const destinations = await getWebFolderDestinations(
        stringArg(request, 0, "library_id"),
      );
      const sourceId = stringArg(request, 1, "source_id");
      const sourceParentId = nullableStringArg(request, 2, "source_parent_id");
      if (sourceParentId === null)
        return selectFolderDestinations(destinations, {
          kind: "book",
          source_id: sourceId,
        });
      const source = destinations.find((destination) => destination.id === sourceId);
      if (!source) throw new Error("The folder is no longer available");
      return selectFolderDestinations(destinations, {
        kind: "move_folder",
        source_parent_id: sourceParentId,
        source_path: source.path,
      });
    }
    case "rename_folder": {
      const name = await renameWebFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
        stringArg(request, 2, "name"),
      );
      scheduleMutationSync();
      return name;
    }
    case "move_folder": {
      const name = await moveWebFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "directory_id"),
        stringArg(request, 2, "destination_id"),
      );
      scheduleMutationSync();
      return name;
    }
    case "copy_book_to_folder": {
      const name = await copyWebBookToFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
        stringArg(request, 2, "source_id"),
        stringArg(request, 3, "destination_id"),
      );
      scheduleMutationSync();
      return name;
    }
    case "move_book_to_folder": {
      const name = await moveWebBookToFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
        stringArg(request, 2, "source_id"),
        stringArg(request, 3, "destination_id"),
      );
      scheduleMutationSync();
      return name;
    }
    case "remove_book_from_folder": {
      const orphaned = await removeWebBookFromFolder(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
        stringArg(request, 2, "source_id"),
      );
      scheduleMutationSync();
      return orphaned;
    }
    case "get_library_authors":
      return getWebLibraryAuthors(
        stringArg(request, 0, "library_id"),
        nullableStringArg(request, 1, "author_id"),
        (request.args[2] as AuthorSort | null) ?? "name",
      );
    case "search_library":
      return searchWebLibrary(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "query"),
        numberArg(request, 2, "limit"),
      );
    case "search_library_folder":
      return searchWebLibrary(
        stringArg(request, 0, "library_id"),
        stringArg(request, 2, "query"),
        numberArg(request, 3, "limit"),
        stringArg(request, 1, "directory_id"),
        fileTypeArg(request, 4),
      );
    case "search_library_subject":
      return searchWebLibrarySubject(
        stringArg(request, 0, "library_id"),
        nullableStringArg(request, 1, "subject_path"),
        stringArg(request, 2, "query"),
        numberArg(request, 3, "limit"),
        fileTypeArg(request, 4),
      );
    case "import_book": {
      const destination = request.args[0];
      const file = request.args[1];
      if (!destination || typeof destination !== "object")
        throw new Error("destination is invalid");
      if (!(file instanceof File)) throw new Error("file is invalid");
      const cover = request.args[3];
      if (cover !== null && cover !== undefined && !(cover instanceof Uint8Array))
        throw new Error("cover must contain bytes");
      await importWebBook(
        destination as { library_id: string; directory_id: string },
        file,
        stringArg(request, 2, "file_name"),
        undefined,
        cover as Uint8Array | null | undefined,
      );
      scheduleMutationSync();
      return null;
    }
    case "open_book_blob":
      return openWebBook(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "get_book_file_name":
      return getWebBookFileName(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "get_book_thumbnail":
      return getWebBookThumbnail(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "get_book_reading_position":
      return getWebReadingPosition(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "get_pdf_reading_position":
      return getWebPdfReadingPosition(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "get_audiobook_chapters":
      return getWebAudiobookChapters(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
      );
    case "update_reading_position":
      await updateWebReadingPosition(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
        stringArg(request, 2, "cfi"),
        nullableNumberArg(request, 3, "progress"),
      );
      scheduleMutationSync();
      return null;
    case "update_pdf_reading_position":
      await updateWebPdfReadingPosition(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
        numberArg(request, 2, "page"),
        numberArg(request, 3, "full_page_position"),
      );
      scheduleMutationSync();
      return null;
    case "get_annotations":
    case "get_bookmarks":
      return [];
    default:
      throw new Error(`The web backend does not implement ${request.command} yet`);
  }
}

const databaseMutationCommands = new Set([
  "login",
  "verify_email",
  "logout",
  "bootstrap_remote_libraries",
  "reconcile_libraries",
  "sync_all_libraries",
  "update_reader_preference",
  "update_browsing_preference",
  "get_libraries",
  "create_library",
  "remove_library",
  "move_book_to_trash",
  "move_folder_to_trash",
  "restore_trashed_book",
  "purge_trashed_book",
  "restore_trashed_folder",
  "purge_trashed_folder",
  "empty_library_trash",
  "create_directory",
  "rename_folder",
  "move_folder",
  "copy_book_to_folder",
  "move_book_to_folder",
  "remove_book_from_folder",
  "import_book",
  "import_catalog_book",
  "update_reading_position",
  "update_pdf_reading_position",
]);

type WorkerEndpoint = {
  addEventListener(
    type: "message",
    listener: (event: MessageEvent<WorkerCommandRequest>) => void,
  ): void;
  postMessage(message: WorkerCommandResponse): void;
  start?: () => void;
};

function attachEndpoint(endpoint: WorkerEndpoint): void {
  endpoint.addEventListener(
    "message",
    async (event: MessageEvent<WorkerCommandRequest>) => {
      const request = event.data;
      if (request?.kind !== "command") return;

      let response: WorkerCommandResponse;
      try {
        const value = await executeCommand(request);
        if (databaseMutationCommands.has(request.command))
          await persistWebDatabaseFallback();
        response = { kind: "response", id: request.id, ok: true, value };
      } catch (reason) {
        response = {
          kind: "response",
          id: request.id,
          ok: false,
          error: reason instanceof Error ? reason.message : String(reason),
        };
      }
      endpoint.postMessage(response);
    },
  );
  endpoint.start?.();
}

type WorkerScope = WorkerEndpoint & {
  addEventListener(type: "connect", listener: (event: MessageEvent) => void): void;
  onconnect?: ((event: MessageEvent) => void) | null;
};

const workerScope = self as unknown as WorkerScope;
if ("onconnect" in workerScope) {
  workerScope.addEventListener("connect", (event: MessageEvent) => {
    const port = event.ports[0];
    if (port) attachEndpoint(port);
  });
} else {
  attachEndpoint(workerScope);
}

export {};
