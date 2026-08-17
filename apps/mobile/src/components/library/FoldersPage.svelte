<script lang="ts">
  import type { Snippet } from "svelte";
  import FolderCard from "./FolderCard.svelte";
  import BookGrid from "./BookGrid.svelte";
  import BrowseNavigationGrid from "./BrowseNavigationGrid.svelte";
  import ContextualLibrarySearch from "./ContextualLibrarySearch.svelte";
  import FileTypeFilter from "./FileTypeFilter.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import { BROWSING_CONTENT_CONTRACT, BROWSING_STATES_CONTRACT } from "../../lib/types";
  import type { BookCardData, DownloadState, FolderDownloadStatus, LibraryDirectory, LibraryFileTypeFilter } from "../../lib/types";

  let { libraryId, folders, books, folderPath, fileType, query = $bindable(), detailed, openingBookId, downloadStates, folderDownloadStatuses, activeFolderDownloads, searchResults, onsearch, onfiletype, onopenfolder, ondownloadfolder, ontrashfolder, onrenamefolder, onmovefolder, onopen, ondownload, onremove, ontrash, oncopybook, onmovebook, onremovebookfromfolder, onadd, onnewfolder }: {
    libraryId: string; folders: LibraryDirectory[]; books: BookCardData[]; folderPath: { id: string; name: string }[]; query: string; detailed: boolean; openingBookId: string;
    downloadStates: Record<string, DownloadState>; folderDownloadStatuses: Record<string, FolderDownloadStatus>; activeFolderDownloads: Record<string, boolean>;
    searchResults: Snippet; onsearch: () => void; onopenfolder: (folder: LibraryDirectory) => void | Promise<void>; ondownloadfolder: (folder: LibraryDirectory) => void | Promise<void>; ontrashfolder: (folder: LibraryDirectory) => void | Promise<void>;
    onrenamefolder: (folder: LibraryDirectory) => void | Promise<void>; onmovefolder: (folder: LibraryDirectory) => void | Promise<void>;
    onopen: (book: BookCardData) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrash: (book: BookCardData) => void | Promise<void>;
    oncopybook: (book: BookCardData) => void | Promise<void>; onmovebook: (book: BookCardData) => void | Promise<void>; onremovebookfromfolder: (book: BookCardData) => void | Promise<void>;
    fileType: LibraryFileTypeFilter; onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>; onadd: () => void | Promise<void>; onnewfolder: () => void;
  } = $props();
  const placeholder = $derived(folderPath.length ? `Search books or authors in ${folderPath.at(-1)?.name} and subfolders` : "Search books or authors");
  const selectedTypeLabel = $derived(BROWSING_CONTENT_CONTRACT.file_type_filters.find((option) => option.value === fileType)?.label ?? "books");
</script>

<section class="browse-page" aria-label="Folders">
  <ContextualLibrarySearch bind:value={query} {placeholder} oninput={onsearch} />
  <FileTypeFilter value={fileType} onchange={onfiletype} />
  {#if query.trim()}
    {@render searchResults()}
  {:else}
    {#if folders.length}<BrowseNavigationGrid>{#each folders.filter((folder) => folder.id !== "__root__") as folder}<FolderCard {folder} status={folderDownloadStatuses[folder.id]} active={activeFolderDownloads[folder.id]} onopen={onopenfolder} ondownload={ondownloadfolder} ontrash={ontrashfolder} onrename={onrenamefolder} onmove={onmovefolder} />{/each}</BrowseNavigationGrid>{/if}
    <BookGrid {libraryId} {books} {detailed} emptyMessage="" {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} oncopytofolder={oncopybook} onmovetofolder={onmovebook} onremovefromfolder={onremovebookfromfolder} />
    {#if !folders.length && !books.length}<EmptyPage title={fileType === "all" ? (folderPath.length ? BROWSING_STATES_CONTRACT.empty_folder_title : BROWSING_STATES_CONTRACT.empty_library_title) : `No ${selectedTypeLabel.toLowerCase()} here`} description={fileType === "all" ? (folderPath.length ? BROWSING_STATES_CONTRACT.empty_folder_description : BROWSING_STATES_CONTRACT.empty_library_description) : `This folder and its subfolders contain no ${selectedTypeLabel.toLowerCase()}.`} primaryLabel="Add book" secondaryLabel="New folder" onprimary={onadd} onsecondary={onnewfolder} />{/if}
  {/if}
</section>

<style>
  .browse-page { width: 100%; }
</style>
