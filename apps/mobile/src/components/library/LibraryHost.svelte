<script lang="ts">
  import BookGrid from "./BookGrid.svelte";
  import HomePage from "./HomePage.svelte";
  import AuthorsPage from "./AuthorsPage.svelte";
  import FoldersPage from "./FoldersPage.svelte";
  import SubjectsPage from "./SubjectsPage.svelte";
  import TrashPage from "./TrashPage.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import { BROWSING_STATES_CONTRACT, type AuthorSort, type AuthorSummary, type BookCardData, type BrowsingPreferences, type DirectoryContent, type DownloadState, type FolderDownloadStatus, type LibraryDirectory, type LibraryFileTypeFilter } from "../../lib/types";
  import type { LibraryTrashView, TrashedBook, TrashedFolder } from "../../lib/bindings";
  import type { Destination } from "../../lib/navigation";

  type Level = { id: string; name: string };
  let {
    destination, configured, loading, libraryId, preferences, inProgress, finished, recentlyAdded,
    directoryContent, folderPath, subjectContent, subjectPath, fileType, subjectsLoading, authors,
    authorFilter = $bindable(), authorSort, focusedAuthorIndex = $bindable(), selectedAuthorId, selectedAuthorName, authorBooks,
    searchQuery = $bindable(), searchPending, searchCompletedQuery, titleSearchResults, authorSearchResults,
    trash, trashLoading, trashBusy, trashError, openingBookId, downloadStates, folderDownloadStatuses, activeFolderDownloads, error,
    oncreateLibrary, onadd, onopen, onopenbookhash, ondownload, onremove, ontrashbook, oncopybook, onmovebook, onremovebookfromfolder, onopenfolder, ondownloadfolder, ontrashfolder, onrenamefolder, onmovefolder,
    onnewfolder, onsearch, onfiletype, onopensubject, onfilterkeydown, onsort, onauthorkeydown, onselectauthor, onclearauthor,
    onimported, onretry, onloadtrash, onrestorebook, onpurgebook, onrestorefolder, onpurgefolder, onemptytrash,
  }: {
    destination: Destination; configured: boolean; loading: boolean; libraryId: string; preferences: BrowsingPreferences;
    inProgress: BookCardData[]; finished: BookCardData[]; recentlyAdded: BookCardData[]; directoryContent: DirectoryContent; folderPath: Level[];
    subjectContent: DirectoryContent; subjectPath: Level[]; fileType: LibraryFileTypeFilter; subjectsLoading: boolean; authors: AuthorSummary[]; authorFilter: string; authorSort: AuthorSort;
    focusedAuthorIndex: number; selectedAuthorId: string; selectedAuthorName: string; authorBooks: BookCardData[]; searchQuery: string; searchPending: boolean;
    searchCompletedQuery: string; titleSearchResults: BookCardData[]; authorSearchResults: BookCardData[]; trash: LibraryTrashView; trashLoading: boolean; trashBusy: boolean;
    trashError: string; openingBookId: string; downloadStates: Record<string, DownloadState>; folderDownloadStatuses: Record<string, FolderDownloadStatus>; activeFolderDownloads: Record<string, boolean>; error: string;
    oncreateLibrary: () => void; onadd: () => void | Promise<void>; onopen: (book: BookCardData) => void | Promise<void>; onopenbookhash: (contentHash: string) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrashbook: (book: BookCardData) => void | Promise<void>;
    oncopybook: (book: BookCardData) => void | Promise<void>; onmovebook: (book: BookCardData) => void | Promise<void>; onremovebookfromfolder: (book: BookCardData) => void | Promise<void>;
    onopenfolder: (folder: LibraryDirectory) => void | Promise<void>; ondownloadfolder: (folder: LibraryDirectory) => void | Promise<void>; ontrashfolder: (folder: LibraryDirectory) => void | Promise<void>; onrenamefolder: (folder: LibraryDirectory) => void | Promise<void>; onmovefolder: (folder: LibraryDirectory) => void | Promise<void>; onnewfolder: () => void; onsearch: () => void; onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>; onopensubject: (subject: LibraryDirectory) => void | Promise<void>;
    onfilterkeydown: (event: KeyboardEvent) => void; onsort: (sort: AuthorSort) => void | Promise<void>; onauthorkeydown: (event: KeyboardEvent, index: number) => void; onselectauthor: (id: string, name: string) => void | Promise<void>; onclearauthor: () => void; onimported: () => void | Promise<void>; onretry: () => void | Promise<void>; onloadtrash: () => void | Promise<void>;
    onrestorebook: (book: TrashedBook) => void | Promise<void>; onpurgebook: (book: TrashedBook) => void | Promise<void>; onrestorefolder: (folder: TrashedFolder) => void | Promise<void>; onpurgefolder: (folder: TrashedFolder) => void | Promise<void>; onemptytrash: () => void | Promise<void>;
  } = $props();
  const detailed = $derived(preferences.library_view === "detailed");
</script>

{#snippet searchResults()}
  <div class="scoped-search-results" aria-live="polite">
    {#if searchPending}<div class="inline-empty">Searching…</div>
    {:else if searchCompletedQuery}
      {#if titleSearchResults.length}<h2>Title matches</h2><BookGrid {libraryId} books={titleSearchResults} {detailed} emptyMessage="" {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} ontrash={ontrashbook} />{/if}
      {#if authorSearchResults.length}<h2>Author matches</h2><BookGrid {libraryId} books={authorSearchResults} {detailed} emptyMessage="" {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} ontrash={ontrashbook} />{/if}
      {#if !titleSearchResults.length && !authorSearchResults.length}<div class="inline-empty">{BROWSING_STATES_CONTRACT.no_search_results_label}</div>{/if}
    {/if}
  </div>
{/snippet}

{#if destination === "trash"}
  <TrashPage contents={trash} loading={trashLoading} busy={trashBusy} error={trashError} onretry={onloadtrash} {onrestorebook} {onpurgebook} {onrestorefolder} {onpurgefolder} onempty={onemptytrash} />
{:else if loading}
  <section class="page-state" aria-live="polite">{BROWSING_STATES_CONTRACT.loading_library_label}</section>
{:else if !configured}
  <EmptyPage title={BROWSING_STATES_CONTRACT.no_libraries_title} description={BROWSING_STATES_CONTRACT.no_libraries_description} primaryLabel={BROWSING_STATES_CONTRACT.create_library_label} onprimary={oncreateLibrary} />
{:else if destination === "home"}
  <HomePage {libraryId} {inProgress} {finished} {recentlyAdded} {openingBookId} {downloadStates} {onadd} {onopen} {ondownload} {onremove} ontrash={ontrashbook} />
{:else if destination === "folders"}
  <FoldersPage {libraryId} folders={directoryContent.children} books={directoryContent.books} folderPath={folderPath} {fileType} bind:query={searchQuery} {detailed} {openingBookId} {downloadStates} {folderDownloadStatuses} {activeFolderDownloads} {searchResults} {onsearch} {onfiletype} {onopenfolder} {ondownloadfolder} {ontrashfolder} {onrenamefolder} {onmovefolder} {onopen} {ondownload} {onremove} ontrash={ontrashbook} {oncopybook} {onmovebook} {onremovebookfromfolder} {onadd} {onnewfolder} />
{:else if destination === "authors"}
  <AuthorsPage {libraryId} {authors} bind:filter={authorFilter} sort={authorSort} bind:focusedIndex={focusedAuthorIndex} {selectedAuthorId} {selectedAuthorName} books={authorBooks} {detailed} {openingBookId} {downloadStates} {onfilterkeydown} {onsort} {onauthorkeydown} {onselectauthor} onclear={onclearauthor} {onadd} {onimported} {onopen} onopenbook={onopenbookhash} {ondownload} {onremove} ontrash={ontrashbook} />
{:else if destination === "subjects"}
  <SubjectsPage {libraryId} subjects={subjectContent.children} books={subjectContent.books} subjectPath={subjectPath} {fileType} loading={subjectsLoading} bind:query={searchQuery} {detailed} {openingBookId} {downloadStates} {searchResults} {onsearch} {onfiletype} {onopensubject} {onimported} {onopen} {ondownload} {onremove} ontrash={ontrashbook} />
{/if}

{#if error}<div class="error-banner" role="alert"><span>Library unavailable: {error}</span><button type="button" onclick={() => void onretry()}>{BROWSING_STATES_CONTRACT.retry_label}</button></div>{/if}

<style>
  .page-state { display: grid; place-items: center; min-height: 240px; padding: 18px; color: var(--color-subtle-text); font-size: .8125rem; text-align: center; }
  .inline-empty { display: grid; min-height: 5rem; place-items: center; justify-items: start; color: var(--color-subtle-text); font-size: .84rem; text-align: left; }
  .scoped-search-results { display: grid; gap: .75rem; } .scoped-search-results h2 { margin: .35rem 0 0; font: 600 1rem var(--font-interface-family); }
  .error-banner { display: flex; align-items: center; justify-content: space-between; gap: 1rem; margin: 1rem 0; padding: .75rem 1rem; border-radius: 0; color: var(--color-danger-text); background: var(--color-danger-bg); font-size: .75rem; overflow-wrap: anywhere; }
  .error-banner button { flex: none; min-height: 2.2rem; padding: .4rem .7rem; border: 1px solid currentColor; border-radius: 0; color: inherit; background: transparent; font: inherit; font-weight: 750; cursor: pointer; }
</style>
