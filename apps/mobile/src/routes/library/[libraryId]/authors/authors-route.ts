import { writable } from "svelte/store";
import * as bookActions from "../../../../lib/library/book-actions";
import { commands } from "../../../../lib/app/commands";
import { libraryBookHref, libraryFolderHref } from "../../../../lib/app/navigation";
import type { AuthorSummary, DownloadState } from "../../../../lib/bindings";
import type { BookCardData } from "../../../../lib/shared/types";

export interface AuthorsRouteState {
  libraryId: string;
  loading: boolean;
  error: string;
  authors: AuthorSummary[];
  filter: string;
  focusedIndex: number;
  selectedAuthorId: string;
  selectedAuthorName: string;
  books: BookCardData[];
  detailed: boolean;
  downloadStates: Record<string, DownloadState>;
}

const initialState = (): AuthorsRouteState => ({
  libraryId: "",
  loading: false,
  error: "",
  authors: [],
  filter: "",
  focusedIndex: 0,
  selectedAuthorId: "",
  selectedAuthorName: "",
  books: [],
  detailed: false,
  downloadStates: {},
});

function errorMessage(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason);
}

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

export function createAuthorsRoute(navigate: (path: string) => Promise<void>) {
  const store = writable<AuthorsRouteState>(initialState());
  let state = initialState();
  const { subscribe } = store;
  const update = (mutate: (current: AuthorsRouteState) => AuthorsRouteState) => {
    state = mutate(state);
    store.set(state);
  };
  let libraryId = "";
  let request = 0;

  const visibleAuthors = () => {
    const query = state.filter.trim().toLocaleLowerCase();
    return query
      ? state.authors.filter((author) => author.name.toLocaleLowerCase().includes(query))
      : state.authors;
  };

  function focusAuthor(index: number) {
    const authors = visibleAuthors();
    if (!authors.length) return;
    const focusedIndex = Math.max(0, Math.min(index, authors.length - 1));
    update((current) => ({ ...current, focusedIndex }));
    requestAnimationFrame(() =>
      document
        .querySelector<HTMLButtonElement>(`[data-author-index="${focusedIndex}"]`)
        ?.focus(),
    );
  }

  async function load(id: string, force = false) {
    if (!id || (!force && id === libraryId)) return;
    libraryId = id;
    const current = ++request;
    update((currentState) => ({
      ...currentState,
      libraryId: id,
      loading: true,
      error: "",
      selectedAuthorId: "",
      selectedAuthorName: "",
      books: [],
    }));
    try {
      const [view, preferences] = await Promise.all([
        commands.getLibraryAuthors(id, null, "name"),
        commands.getBrowsingPreferences(),
      ]);
      if (current !== request) return;
      update((currentState) => ({
        ...currentState,
        loading: false,
        authors: view.authors,
        detailed: preferences.library_view === "detailed",
      }));
    } catch (reason) {
      if (current !== request) return;
      update((currentState) => ({
        ...currentState,
        loading: false,
        error: errorMessage(reason),
      }));
    }
  }

  async function select(id: string, name: string) {
    const current = ++request;
    update((currentState) => ({ ...currentState, error: "" }));
    try {
      const view = await commands.getLibraryAuthors(libraryId, id, "name");
      if (current !== request) return;
      const books = view.books ?? [];
      update((currentState) => ({
        ...currentState,
        selectedAuthorId: id,
        selectedAuthorName: name,
        books,
        downloadStates: downloadStatesFor(books),
      }));
    } catch (reason) {
      if (current !== request) return;
      update((currentState) => ({ ...currentState, error: errorMessage(reason) }));
    }
  }

  function setDownloadState(contentHash: string, value: DownloadState) {
    update((s) => ({
      ...s,
      downloadStates: { ...s.downloadStates, [contentHash]: value },
    }));
  }

  async function download(book: BookCardData) {
    try {
      await bookActions.downloadBook(libraryId, book);
    } catch (reason) {
      update((current) => ({ ...current, error: errorMessage(reason) }));
    }
  }
  async function remove(book: BookCardData) {
    try {
      await bookActions.removeLocalCopy(libraryId, book);
    } catch (reason) {
      update((current) => ({ ...current, error: errorMessage(reason) }));
    }
  }
  async function trash(book: BookCardData) {
    try {
      if (await bookActions.trashBook(libraryId, book)) {
      }
    } catch (reason) {
      update((current) => ({ ...current, error: errorMessage(reason) }));
    }
  }

  return {
    subscribe,
    load,
    setDownloadState,
    select,
    clear() {
      request += 1;
      update((current) => ({
        ...current,
        selectedAuthorId: "",
        selectedAuthorName: "",
        books: [],
      }));
      focusAuthor(state.focusedIndex);
    },
    setFilter(filter: string) {
      update((current) => ({ ...current, filter, focusedIndex: 0 }));
    },
    setFocusedIndex(focusedIndex: number) {
      update((current) => ({ ...current, focusedIndex }));
    },
    filterKeydown(event: KeyboardEvent) {
      if (event.key === "ArrowDown" && visibleAuthors().length) {
        event.preventDefault();
        focusAuthor(0);
      } else if (event.key === "Escape" && state.filter) {
        event.preventDefault();
        update((current) => ({ ...current, filter: "", focusedIndex: 0 }));
      }
    },
    authorKeydown(event: KeyboardEvent, index: number) {
      const lastIndex = visibleAuthors().length - 1;
      const nextIndex =
        event.key === "ArrowDown"
          ? index + 1
          : event.key === "ArrowUp"
            ? index - 1
            : event.key === "Home"
              ? 0
              : event.key === "End"
                ? lastIndex
                : undefined;
      if (nextIndex === undefined) return;
      event.preventDefault();
      focusAuthor(nextIndex);
    },
    open: (book: BookCardData) => navigate(libraryBookHref(libraryId, book.content_hash)),
    openBook: (contentHash: string) => navigate(libraryBookHref(libraryId, contentHash)),
    add: () => navigate(libraryFolderHref(libraryId)),
    download,
    remove,
    trash,
  };
}
