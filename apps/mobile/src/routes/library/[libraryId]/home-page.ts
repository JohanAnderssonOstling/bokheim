import { writable } from "svelte/store";
import * as bookActions from "../../../lib/library/book-actions";
import { commands } from "../../../lib/app/commands";
import { libraryBookHref, libraryFolderHref } from "../../../lib/app/navigation";
import {
  BROWSING_CONTENT_CONTRACT,
  type BookCardData,
  type DownloadState,
} from "../../../lib/shared/types";

export interface HomePageState {
  libraryId: string;
  loading: boolean;
  error: string;
  inProgress: BookCardData[];
  finished: BookCardData[];
  recentlyAdded: BookCardData[];
  downloadStates: Record<string, DownloadState>;
}

const initialState = (): HomePageState => ({
  libraryId: "",
  loading: false,
  error: "",
  inProgress: [],
  finished: [],
  recentlyAdded: [],
  downloadStates: {},
});

function downloadStatesFor(books: BookCardData[]): Record<string, DownloadState> {
  return Object.fromEntries(
    books.map((book) => [
      book.content_hash,
      book.downloaded
        ? "Downloaded"
        : book.download_requested
          ? "Queued"
          : "NotDownloaded",
    ]),
  );
}

export function createHomePageController(navigate: (path: string) => Promise<void>) {
  const { subscribe, update } = writable<HomePageState>(initialState());
  let request = 0;
  let activeLibraryId = "";

  async function load(libraryId: string, force = false) {
    if (!libraryId) return;
    activeLibraryId = libraryId;
    const current = ++request;
    let shouldLoad = force;
    update((state) => {
      shouldLoad ||= state.libraryId !== libraryId;
      return shouldLoad ? { ...state, libraryId, loading: true, error: "" } : state;
    });
    if (!shouldLoad) return;
    try {
      const home = await commands.getLibraryHome(
        libraryId,
        BROWSING_CONTENT_CONTRACT.home_book_limit,
      );
      if (current !== request) return;
      const recentlyRead = home.recently_read;
      const allBooks = [...home.recently_added, ...recentlyRead];
      update((state) => ({
        ...state,
        loading: false,
        inProgress: recentlyRead.filter(
          (book) =>
            (book.progress ?? 0) < BROWSING_CONTENT_CONTRACT.finished_progress_threshold,
        ),
        finished: recentlyRead.filter(
          (book) =>
            (book.progress ?? 0) >= BROWSING_CONTENT_CONTRACT.finished_progress_threshold,
        ),
        recentlyAdded: home.recently_added,
        downloadStates: downloadStatesFor(allBooks),
      }));
    } catch (reason) {
      if (current !== request) return;
      update((state) => ({
        ...state,
        loading: false,
        error: reason instanceof Error ? reason.message : String(reason),
      }));
    }
  }

  function setDownloadState(contentHash: string, value: DownloadState) {
    update((s) => ({
      ...s,
      downloadStates: { ...s.downloadStates, [contentHash]: value },
    }));
  }
  async function open(book: BookCardData) {
    await navigate(libraryBookHref(activeLibraryId, book.content_hash));
  }

  async function download(book: BookCardData) {
    try {
      await bookActions.downloadBook(activeLibraryId, book);
    } catch (reason) {
      update((state) => ({
        ...state,
        error: reason instanceof Error ? reason.message : String(reason),
      }));
    }
  }
  async function remove(book: BookCardData) {
    try {
      await bookActions.removeLocalCopy(activeLibraryId, book);
    } catch (reason) {
      update((state) => ({
        ...state,
        error: reason instanceof Error ? reason.message : String(reason),
      }));
    }
  }
  async function trash(book: BookCardData) {
    try {
      if (await bookActions.trashBook(activeLibraryId, book)) {
      }
    } catch (reason) {
      update((state) => ({
        ...state,
        error: reason instanceof Error ? reason.message : String(reason),
      }));
    }
  }

  return {
    subscribe,
    load,
    setDownloadState,
    open,
    download,
    remove,
    trash,
    add: () => navigate(libraryFolderHref(activeLibraryId)),
  };
}
