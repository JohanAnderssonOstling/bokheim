/// <reference lib="webworker" />

import type { AuthorSort, ReaderPreferencePatch } from "../lib/bindings";
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
import { bootstrapWebRemoteLibraries, reconcileWebLibraryCatalogue } from "../lib/web/catalogue-sync";
import { synchronizeAllWebLibraries } from "../lib/web/library-sync";
import { downloadWebBook, getWebLocalStorageUsage, getWebTransferStatuses } from "../lib/web/blob-sync";
import { getWebDatabaseHealth } from "../lib/web/database";
import {
  createWebLibrary,
  createWebDirectory,
  getWebChildDirectories,
  getWebDirectoryPath,
  getWebBookFileName,
  getWebBookThumbnail,
  getWebLibraryHierarchy,
  getWebLibraryAuthors,
  getWebLibraryFolder,
  getWebLibraryHome,
  getWebLibrarySubjects,
  getWebReadingPosition,
  importWebBook,
  listWebLibraries,
  openWebBook,
  removeWebLibrary,
  searchWebLibrary,
  updateWebReadingPosition,
} from "../lib/web/library-database";
import type { WorkerCommandRequest, WorkerCommandResponse } from "../lib/web/worker-protocol";
import { getWebReaderPreferences, updateWebReaderPreference } from "../lib/web/preferences-database";

function stringArg(request: WorkerCommandRequest, index: number, name: string): string {
  const value = request.args[index];
  if (typeof value !== "string") throw new Error(`${name} must be a string`);
  return value;
}

function nullableStringArg(request: WorkerCommandRequest, index: number, name: string): string | null {
  const value = request.args[index];
  if (value === null) return null;
  if (typeof value !== "string") throw new Error(`${name} must be a string or null`);
  return value;
}

function nullableNumberArg(request: WorkerCommandRequest, index: number, name: string): number | null {
  const value = request.args[index];
  if (value === null) return null;
  if (typeof value !== "number" || !Number.isFinite(value)) throw new Error(`${name} must be a finite number or null`);
  return value;
}

function numberArg(request: WorkerCommandRequest, index: number, name: string): number {
  const value = nullableNumberArg(request, index, name);
  if (value === null) throw new Error(`${name} must be a number`);
  return value;
}

function objectArg<T extends object>(request: WorkerCommandRequest, index: number, name: string): T {
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
      return loginWebAccount(stringArg(request, 0, "email"), stringArg(request, 1, "password"));
    case "request_public_registration":
      return registerWebAccount(stringArg(request, 0, "email"), stringArg(request, 1, "password"));
    case "resend_verification":
      return resendWebVerification(stringArg(request, 0, "email"));
    case "verify_email":
      return verifyWebEmail(stringArg(request, 0, "token"));
    case "request_password_reset":
      return requestWebPasswordReset(stringArg(request, 0, "email"));
    case "reset_password":
      return resetWebPassword(stringArg(request, 0, "token"), stringArg(request, 1, "new_password"));
    case "logout":
      return logoutWebAccount();
    case "bootstrap_remote_libraries":
      await bootstrapWebRemoteLibraries();
      await synchronizeAllWebLibraries();
      return listWebLibraries();
    case "reconcile_library_catalogue":
      return reconcileWebLibraryCatalogue();
    case "sync_all_libraries":
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
      return updateWebReaderPreference(objectArg<ReaderPreferencePatch>(request, 0, "patch"));
    case "get_libraries":
      return listWebLibraries();
    case "create_library":
      return createWebLibrary(stringArg(request, 0, "name"));
    case "remove_library":
      return removeWebLibrary(stringArg(request, 0, "library_id"));
    case "get_library_home":
      return getWebLibraryHome(stringArg(request, 0, "library_id"));
    case "get_library_folder":
      return getWebLibraryFolder(stringArg(request, 0, "library_id"), stringArg(request, 1, "directory_id"));
    case "get_library_subjects":
      return getWebLibrarySubjects(stringArg(request, 0, "library_id"), nullableStringArg(request, 1, "subject_path"));
    case "get_child_directories":
      return getWebChildDirectories(stringArg(request, 0, "library_id"), stringArg(request, 1, "parent_id"));
    case "get_library_folder_path":
      return getWebDirectoryPath(stringArg(request, 0, "library_id"), stringArg(request, 1, "directory_id"));
    case "create_directory":
      return createWebDirectory(
        stringArg(request, 0, "library_id"), stringArg(request, 1, "parent_id"), stringArg(request, 2, "name"),
      );
    case "get_library_authors":
      return getWebLibraryAuthors(stringArg(request, 0, "library_id"), nullableStringArg(request, 1, "author_id"), (request.args[2] as AuthorSort | null) ?? "name");
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
      );
    case "search_library_subject":
      return searchWebLibrary(
        stringArg(request, 0, "library_id"),
        stringArg(request, 2, "query"),
        numberArg(request, 3, "limit"),
      );
    case "import_book": {
      const destination = request.args[0];
      const file = request.args[1];
      if (!destination || typeof destination !== "object") throw new Error("destination is invalid");
      if (!(file instanceof File)) throw new Error("file is invalid");
      return importWebBook(destination as { library_id: string; directory_id: string }, file, stringArg(request, 2, "file_name"));
    }
    case "open_book_blob":
      return openWebBook(stringArg(request, 0, "library_id"), stringArg(request, 1, "content_hash"));
    case "download_book":
      return downloadWebBook(stringArg(request, 0, "library_id"), stringArg(request, 1, "content_hash"));
    case "get_book_file_name":
      return getWebBookFileName(stringArg(request, 0, "library_id"), stringArg(request, 1, "content_hash"));
    case "get_book_thumbnail":
      return getWebBookThumbnail(stringArg(request, 0, "library_id"), stringArg(request, 1, "content_hash"));
    case "get_book_reading_position":
      return getWebReadingPosition(stringArg(request, 0, "library_id"), stringArg(request, 1, "content_hash"));
    case "update_reading_position":
      return updateWebReadingPosition(
        stringArg(request, 0, "library_id"),
        stringArg(request, 1, "content_hash"),
        stringArg(request, 2, "cfi"),
        nullableNumberArg(request, 3, "progress"),
      );
    case "get_annotations":
    case "get_bookmarks":
      return [];
    default:
      throw new Error(`The web backend does not implement ${request.command} yet`);
  }
}

self.addEventListener("message", async (event: MessageEvent<WorkerCommandRequest>) => {
  const request = event.data;
  if (request?.kind !== "command") return;

  let response: WorkerCommandResponse;
  try {
    response = { kind: "response", id: request.id, ok: true, value: await executeCommand(request) };
  } catch (reason) {
    response = {
      kind: "response",
      id: request.id,
      ok: false,
      error: reason instanceof Error ? reason.message : String(reason),
    };
  }
  self.postMessage(response);
});

export {};
