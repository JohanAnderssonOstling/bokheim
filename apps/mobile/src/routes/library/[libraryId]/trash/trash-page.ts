import { writable } from "svelte/store";
import { requestLibraryRefresh } from "../../../../lib/app/library-events";
import { commands } from "../../../../lib/app/commands";
import type {
  LibraryTrashView,
  TrashedBook,
  TrashedFolder,
} from "../../../../lib/bindings";

const emptyTrash = (): LibraryTrashView => ({ books: [], folders: [] });

interface TrashPageState {
  libraryId: string;
  contents: LibraryTrashView;
  loading: boolean;
  busy: boolean;
  error: string;
}

export function createTrashPageController() {
  const { subscribe, update } = writable<TrashPageState>({
    libraryId: "",
    contents: emptyTrash(),
    loading: false,
    busy: false,
    error: "",
  });
  let request = 0;
  let libraryId = "";

  const message = (reason: unknown) =>
    reason instanceof Error ? reason.message : String(reason);

  async function load(nextLibraryId: string, force = false) {
    if (!nextLibraryId) return;
    if (nextLibraryId !== libraryId) force = true;
    libraryId = nextLibraryId;
    const current = ++request;
    update((state) =>
      force ? { ...state, libraryId, loading: true, error: "" } : state,
    );
    if (!force) return;
    try {
      const contents = await commands.getLibraryTrash(libraryId);
      if (current !== request) return;
      update((state) => ({ ...state, loading: false, contents }));
    } catch (reason) {
      if (current !== request) return;
      update((state) => ({ ...state, loading: false, error: message(reason) }));
    }
  }

  async function run(action: () => Promise<unknown>) {
    update((state) => ({ ...state, busy: true, error: "" }));
    try {
      await action();
      // Restoring returns a book to the listings and purging removes it for
      // good, so every page re-reads — including this one, which listens too.
      requestLibraryRefresh();
    } catch (reason) {
      update((state) => ({ ...state, error: message(reason) }));
    } finally {
      update((state) => ({ ...state, busy: false }));
    }
  }

  return {
    subscribe,
    load,
    restoreBook: (book: TrashedBook) =>
      run(() => commands.restoreTrashedBook(libraryId, book.content_hash)),
    purgeBook: (book: TrashedBook) =>
      run(() => commands.purgeTrashedBook(libraryId, book.content_hash)),
    restoreFolder: (folder: TrashedFolder) =>
      run(() => commands.restoreTrashedFolder(libraryId, folder.id)),
    purgeFolder: (folder: TrashedFolder) =>
      run(() => commands.purgeTrashedFolder(libraryId, folder.id)),
    empty: () => run(() => commands.emptyLibraryTrash(libraryId)),
  };
}
