import { writable } from "svelte/store";
import * as bookActions from "../../../../../lib/library/book-actions";
import { commands } from "../../../../../lib/app/commands";
import { createScopedSearch } from "../../../../../lib/library/scoped-search";
import { libraryBookHref, librarySubjectsHref } from "../../../../../lib/app/navigation";
import type {
  BookCardData,
  DirectoryContent,
  DownloadState,
  LibraryFileTypeFilter,
} from "../../../../../lib/shared/types";
import { BROWSING_CONTENT_CONTRACT } from "../../../../../lib/shared/types";
import { subjectBreadcrumb } from "./subject-page";

export interface SubjectsRouteState {
  libraryId: string;
  subjectId: string;
  loading: boolean;
  error: string;
  content: DirectoryContent;
  subjectPath: ReturnType<typeof subjectBreadcrumb>;
  fileType: LibraryFileTypeFilter;
  query: string;
  searchPending: boolean;
  searchCompletedQuery: string;
  titleSearchResults: BookCardData[];
  authorSearchResults: BookCardData[];
  detailed: boolean;
  downloadStates: Record<string, DownloadState>;
}

const emptyContent: DirectoryContent = { children: [], books: [] };
const initialState = (): SubjectsRouteState => ({
  libraryId: "",
  subjectId: "__root__",
  loading: false,
  error: "",
  content: emptyContent,
  subjectPath: [],
  fileType: "all",
  query: "",
  searchPending: false,
  searchCompletedQuery: "",
  titleSearchResults: [],
  authorSearchResults: [],
  detailed: false,
  downloadStates: {},
});

const errorMessage = (reason: unknown) =>
  reason instanceof Error ? reason.message : String(reason);

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

export function createSubjectsRoute(navigate: (path: string) => Promise<void>) {
  const store = writable<SubjectsRouteState>(initialState());
  const { subscribe } = store;
  let state = initialState();
  let libraryId = "";
  let request = 0;
  const update = (mutate: (current: SubjectsRouteState) => SubjectsRouteState) => {
    state = mutate(state);
    store.set(state);
  };

  async function load(nextLibraryId: string, subjectId = "__root__", force = false) {
    if (!nextLibraryId) return;
    if (!force && libraryId === nextLibraryId && state.subjectId === subjectId) return;
    libraryId = nextLibraryId;
    const current = ++request;
    // The query belongs to the subject being left, not the one being opened.
    search.cancel();
    update((currentState) => ({
      ...currentState,
      libraryId: nextLibraryId,
      subjectId,
      subjectPath: subjectId === "__root__" ? [] : subjectBreadcrumb(subjectId),
      loading: true,
      error: "",
      query: "",
      searchPending: false,
      searchCompletedQuery: "",
      titleSearchResults: [],
      authorSearchResults: [],
    }));
    try {
      const [content, preferences] = await Promise.all([
        commands.getLibrarySubjects(
          nextLibraryId,
          subjectId === "__root__" ? null : subjectId,
          state.fileType,
        ),
        commands.getBrowsingPreferences(),
      ]);
      if (current !== request) return;
      update((currentState) => ({
        ...currentState,
        loading: false,
        content,
        detailed: preferences.library_view === "detailed",
        downloadStates: downloadStatesFor(content.books),
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

  async function setFileType(fileType: LibraryFileTypeFilter) {
    if (fileType === state.fileType) return;
    update((current) => ({ ...current, fileType }));
    await load(libraryId, state.subjectId, true);
  }

  function setQuery(query: string) {
    update((current) => ({ ...current, query }));
  }

  const search = createScopedSearch(
    (query) =>
      commands.searchLibrarySubject(
        libraryId,
        state.subjectId === "__root__" ? null : state.subjectId,
        query,
        BROWSING_CONTENT_CONTRACT.search_result_limit,
        state.fileType,
      ),
    {
      cleared: () =>
        update((current) => ({
          ...current,
          searchPending: false,
          searchCompletedQuery: "",
          titleSearchResults: [],
          authorSearchResults: [],
        })),
      pending: () => update((current) => ({ ...current, searchPending: true })),
      results: (query, results) =>
        update((current) => ({
          ...current,
          searchPending: false,
          searchCompletedQuery: query,
          titleSearchResults: results.title_matches,
          authorSearchResults: results.author_matches,
          downloadStates: {
            ...current.downloadStates,
            ...downloadStatesFor([...results.title_matches, ...results.author_matches]),
          },
        })),
      failed: (reason) =>
        update((current) => ({
          ...current,
          searchPending: false,
          error: errorMessage(reason),
        })),
    },
  );
  const scheduleSearch = () => search.schedule(state.query);

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
    setFileType,
    setQuery,
    scheduleSearch,
    openSubject: (subjectId: string) =>
      navigate(librarySubjectsHref(libraryId, subjectId)),
    goToSubject: (level: number) =>
      navigate(
        librarySubjectsHref(libraryId, state.subjectPath.slice(0, level).at(-1)?.id),
      ),
    open: (book: BookCardData) => navigate(libraryBookHref(libraryId, book.content_hash)),
    download,
    remove,
    trash,
    refresh: () => load(libraryId, state.subjectId, true),
  };
}
