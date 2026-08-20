import { writable } from "svelte/store";
import * as bookActions from "../../../../../lib/library/book-actions";
import { requestLibraryRefresh } from "../../../../../lib/app/library-events";
import { commands } from "../../../../../lib/app/commands";
import {
  libraryAuthorsHref,
  libraryBookHref,
  libraryRootHref,
  librarySubjectsHref,
} from "../../../../../lib/app/navigation";
import { requestReaderOpen } from "../../../../../lib/app/reader-handoff";
import type { BookDetail, LibraryFolderDestination } from "../../../../../lib/bindings";
import type { BookCardData, DownloadState } from "../../../../../lib/shared/types";

type State = {
  libraryId: string;
  loading: boolean;
  error: string;
  detail: BookDetail | null;
  downloadStates: Record<string, DownloadState>;
  pickerDestinations: LibraryFolderDestination[];
  pickerMode: "copy" | "move" | null;
};
const initial = (): State => ({
  libraryId: "",
  loading: false,
  error: "",
  detail: null,
  downloadStates: {},
  pickerDestinations: [],
  pickerMode: null,
});
const errorMessage = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);

export function createBookDetailRoute(
  navigate: (path: string) => Promise<void>,
  back: () => void,
) {
  const store = writable(initial());
  const { subscribe } = store;
  let state = initial();
  let request = 0;
  const update = (fn: (state: State) => State) => {
    state = fn(state);
    store.set(state);
  };
  async function load(libraryId: string, bookId: string) {
    const current = ++request;
    update((s) => ({ ...s, libraryId, loading: true, error: "", detail: null }));
    try {
      const detail = await commands.getBookDetail(libraryId, bookId);
      if (current === request)
        update((s) => ({
          ...s,
          loading: false,
          detail,
          downloadStates: {
            ...s.downloadStates,
            [bookId]: detail.book.downloaded
              ? "Downloaded"
              : detail.book.download_requested
                ? "Queued"
                : "NotDownloaded",
          },
        }));
    } catch (reason) {
      if (current === request)
        update((s) => ({ ...s, loading: false, error: errorMessage(reason) }));
    }
  }
  function setDownloadState(contentHash: string, value: DownloadState) {
    update((s) => ({
      ...s,
      downloadStates: { ...s.downloadStates, [contentHash]: value },
    }));
  }
  async function openPicker(book: BookCardData, pickerMode: "copy" | "move") {
    const detail = state.detail;
    if (!detail?.source_directory_id) return;
    try {
      const pickerDestinations = await commands.getFolderDestinations(
        state.libraryId,
        detail.source_directory_id,
        null,
      );
      update((s) => ({ ...s, pickerMode, pickerDestinations }));
    } catch (reason) {
      update((s) => ({ ...s, error: errorMessage(reason) }));
    }
  }
  async function confirmPicker(destinationId: string) {
    const { detail, pickerMode, libraryId } = state;
    if (!detail || !pickerMode) return;
    try {
      if (pickerMode === "copy")
        await commands.copyBookToFolder(
          libraryId,
          detail.book.content_hash,
          detail.source_directory_id ?? "",
          destinationId,
        );
      else
        await commands.moveBookToFolder(
          libraryId,
          detail.book.content_hash,
          detail.source_directory_id ?? "",
          destinationId,
        );
      update((s) => ({ ...s, pickerMode: null, pickerDestinations: [] }));
      // The book gained or changed a placement, which the folder listings show.
      requestLibraryRefresh();
      if (pickerMode === "move") back();
    } catch (reason) {
      update((s) => ({ ...s, error: errorMessage(reason) }));
    }
  }
  async function download(book: BookCardData) {
    try {
      await bookActions.downloadBook(state.libraryId, book);
    } catch (reason) {
      update((s) => ({ ...s, error: errorMessage(reason) }));
    }
  }
  async function remove(book: BookCardData) {
    try {
      await bookActions.removeLocalCopy(state.libraryId, book);
    } catch (reason) {
      update((s) => ({ ...s, error: errorMessage(reason) }));
    }
  }
  async function trash(book: BookCardData) {
    try {
      if (await bookActions.trashBook(state.libraryId, book)) {
        back();
      }
    } catch (reason) {
      update((s) => ({ ...s, error: errorMessage(reason) }));
    }
  }

  return {
    subscribe,
    load,
    setDownloadState,
    back,
    read: (book: BookCardData, target?: string) => requestReaderOpen(book, target),
    related: (book: BookCardData) =>
      navigate(libraryBookHref(state.libraryId, book.content_hash)),
    author: (id: string, name: string) =>
      navigate(
        `${libraryAuthorsHref(state.libraryId)}?author=${encodeURIComponent(id)}&name=${encodeURIComponent(name)}`,
      ),
    subject: (path: string) => navigate(librarySubjectsHref(state.libraryId, path)),
    download,
    remove,
    trash,
    copy: (book: BookCardData) => openPicker(book, "copy"),
    move: (book: BookCardData) => openPicker(book, "move"),
    confirmPicker,
    closePicker: () =>
      update((s) => ({ ...s, pickerMode: null, pickerDestinations: [] })),
    retry: () => state.detail && load(state.libraryId, state.detail.book.content_hash),
    home: () => navigate(libraryRootHref(state.libraryId)),
  };
}
