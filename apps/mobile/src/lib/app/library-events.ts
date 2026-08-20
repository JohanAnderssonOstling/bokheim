import type { DownloadState, FolderDownloadStatus } from "../shared/types";

const REFRESH_EVENT = "bokheim:refresh-library";
const BOOK_DOWNLOAD_EVENT = "bokheim:book-download-status";
const FOLDER_DOWNLOAD_EVENT = "bokheim:folder-download-status";

export type BookDownloadChange = { contentHash: string; state: DownloadState };

/**
 * Tells whichever library route is mounted that its data is stale.
 *
 * The shell no longer holds page data, so it cannot refetch a page on the
 * page's behalf. It announces the invalidation instead and each route reloads
 * itself; a route that is not mounted simply reloads when it is next visited.
 */
export function requestLibraryRefresh() {
  window.dispatchEvent(new Event(REFRESH_EVENT));
}

export function onLibraryRefresh(handler: () => void) {
  return subscribe(REFRESH_EVENT, handler);
}

/**
 * Backend download progress, republished for the routes.
 *
 * Only the shell registers the Tauri listeners, because they must outlive any
 * single page. Routes own the download state they display, so the shell hands
 * each change on rather than keeping a second copy to render from.
 */
export function publishBookDownloadStatus(change: BookDownloadChange) {
  dispatch(BOOK_DOWNLOAD_EVENT, change);
}

export function onBookDownloadStatus(handler: (change: BookDownloadChange) => void) {
  return subscribeDetail(BOOK_DOWNLOAD_EVENT, handler);
}

export function publishFolderDownloadStatus(change: FolderDownloadStatus) {
  dispatch(FOLDER_DOWNLOAD_EVENT, change);
}

export function onFolderDownloadStatus(handler: (change: FolderDownloadStatus) => void) {
  return subscribeDetail(FOLDER_DOWNLOAD_EVENT, handler);
}

function dispatch<T>(event: string, detail: T) {
  window.dispatchEvent(new CustomEvent<T>(event, { detail }));
}

function subscribe(event: string, handler: () => void) {
  const listener = () => handler();
  window.addEventListener(event, listener);
  return () => window.removeEventListener(event, listener);
}

function subscribeDetail<T>(event: string, handler: (detail: T) => void) {
  const listener = (received: Event) => handler((received as CustomEvent<T>).detail);
  window.addEventListener(event, listener);
  return () => window.removeEventListener(event, listener);
}
