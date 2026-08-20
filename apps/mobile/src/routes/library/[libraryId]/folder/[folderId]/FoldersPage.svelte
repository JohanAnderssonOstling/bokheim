<script lang="ts">
  import type { Snippet } from "svelte";
  import FolderCard from "./FolderCard.svelte";
  import BookGrid from "../../../../../components/shared/books/BookGrid.svelte";
  import BrowseNavigationGrid from "../../../../../components/shared/browse/BrowseNavigationGrid.svelte";
  import BrowseFilterBar from "../../../../../components/shared/browse/BrowseFilterBar.svelte";
  import EmptyPage from "../../../../../components/shell/EmptyPage.svelte";
  import StickyTopbar from "../../../../../components/shared/StickyTopbar.svelte";
  import FolderBreadcrumbs from "../../../../../components/shared/browse/FolderBreadcrumbs.svelte";
  import FileIcon from "../../../../../components/shared/FileIcon.svelte";
  import {
    BROWSING_CONTENT_CONTRACT,
    BROWSING_STATES_CONTRACT,
    LIBRARY_ACTIONS_CONTRACT,
  } from "../../../../../lib/shared/types";
  import { isWebApp } from "../../../../../lib/app/platform";
  import bookOpen from "@icons/phosphor/book-open.svg";
  import folder from "@icons/phosphor/folder.svg";
  import arrowSquareOut from "@icons/phosphor/arrow-square-out.svg";
  import plus from "@icons/phosphor/plus.svg";
  import type {
    BookCardData,
    DownloadState,
    FolderDownloadStatus,
    LibraryDirectory,
    LibraryFileTypeFilter,
  } from "../../../../../lib/shared/types";
  import type { LibraryPageLevel } from "../../../../../lib/library/page-types";

  let {
    libraryId,
    folders,
    books,
    folderPath,
    fileType,
    query = $bindable(),
    detailed,
    openingBookId,
    downloadStates,
    folderDownloadStatuses,
    activeFolderDownloads,
    searchResults,
    onsearch,
    onquerychange,
    onfiletype,
    onopenfolder,
    ondownloadfolder,
    ontrashfolder,
    onrenamefolder,
    onmovefolder,
    onopen,
    ondownload,
    onremove,
    ontrash,
    oncopybook,
    onmovebook,
    onremovebookfromfolder,
    onadd,
    onnewfolder,
    addingBook,
    importingFolder,
    onimportfolder,
    onfolder,
  }: {
    libraryId: string;
    folders: LibraryDirectory[];
    books: BookCardData[];
    folderPath: LibraryPageLevel[];
    query: string;
    detailed: boolean;
    openingBookId: string;
    downloadStates: Record<string, DownloadState>;
    folderDownloadStatuses: Record<string, FolderDownloadStatus>;
    activeFolderDownloads: Record<string, boolean>;
    searchResults: Snippet;
    onsearch: () => void;
    onquerychange: (value: string) => void;
    onopenfolder: (folder: LibraryDirectory) => void | Promise<void>;
    ondownloadfolder: (folder: LibraryDirectory) => void | Promise<void>;
    ontrashfolder: (folder: LibraryDirectory) => void | Promise<void>;
    onrenamefolder: (folder: LibraryDirectory) => void | Promise<void>;
    onmovefolder: (folder: LibraryDirectory) => void | Promise<void>;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
    ontrash: (book: BookCardData) => void | Promise<void>;
    oncopybook: (book: BookCardData) => void | Promise<void>;
    onmovebook: (book: BookCardData) => void | Promise<void>;
    onremovebookfromfolder: (book: BookCardData) => void | Promise<void>;
    fileType: LibraryFileTypeFilter;
    onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>;
    onadd: () => void | Promise<void>;
    onnewfolder: () => void;
    addingBook: boolean;
    importingFolder: boolean;
    onimportfolder: () => void | Promise<void>;
    onfolder: (level: number) => void | Promise<void>;
  } = $props();
  const placeholder = $derived(
    folderPath.length
      ? `Search books or authors in ${folderPath.at(-1)?.name} and subfolders`
      : "Search books or authors",
  );
  const selectedTypeLabel = $derived(
    BROWSING_CONTENT_CONTRACT.file_type_filters.find(
      (option) => option.value === fileType,
    )?.label ?? "books",
  );
</script>

<section
  class="browse-page"
  aria-label="Folders"
>
  <StickyTopbar>
    <div class="context">
      <FolderBreadcrumbs
        levels={folderPath}
        onselect={onfolder}
      />
    </div>
  </StickyTopbar>
  {#snippet actions()}
    <button
      class="icon-action"
      type="button"
      aria-label={LIBRARY_ACTIONS_CONTRACT.add_book_label}
      disabled={addingBook}
      onclick={() => void onadd()}
    >
      <FileIcon source={bookOpen} />
      <FileIcon
        source={plus}
        size="0.72rem"
      />
    </button>
    <button
      class="icon-action"
      type="button"
      aria-label="New folder"
      onclick={onnewfolder}
    >
      <FileIcon source={folder} />
      <FileIcon
        source={plus}
        size="0.72rem"
      />
    </button>
    {#if isWebApp}
      <button
        class="icon-action"
        type="button"
        aria-label="Add folder from ZIP"
        disabled={importingFolder}
        onclick={() => void onimportfolder()}
      >
        <FileIcon source={arrowSquareOut} />
      </button>
    {/if}
  {/snippet}
  <BrowseFilterBar
    bind:query
    {placeholder}
    {fileType}
    {onsearch}
    {onquerychange}
    {onfiletype}
    {actions}
  />
  {#if query.trim()}
    {@render searchResults()}
  {:else}
    {#if folders.length}<BrowseNavigationGrid
        >{#each folders.filter((folder) => folder.id !== "__root__") as folder}<FolderCard
            {folder}
            status={folderDownloadStatuses[folder.id]}
            active={activeFolderDownloads[folder.id]}
            onopen={onopenfolder}
            ondownload={ondownloadfolder}
            ontrash={ontrashfolder}
            onrename={onrenamefolder}
            onmove={onmovefolder}
          />{/each}</BrowseNavigationGrid
      >{/if}
    <BookGrid
      {libraryId}
      {books}
      {detailed}
      emptyMessage=""
      {openingBookId}
      {downloadStates}
      {onopen}
      {ondownload}
      {onremove}
      {ontrash}
      oncopytofolder={oncopybook}
      onmovetofolder={onmovebook}
      onremovefromfolder={onremovebookfromfolder}
    />
    {#if !folders.length && !books.length}<EmptyPage
        title={fileType === "all"
          ? folderPath.length
            ? BROWSING_STATES_CONTRACT.empty_folder_title
            : BROWSING_STATES_CONTRACT.empty_library_title
          : `No ${selectedTypeLabel.toLowerCase()} here`}
        description={fileType === "all"
          ? folderPath.length
            ? BROWSING_STATES_CONTRACT.empty_folder_description
            : BROWSING_STATES_CONTRACT.empty_library_description
          : `This folder and its subfolders contain no ${selectedTypeLabel.toLowerCase()}.`}
        primaryLabel="Add book"
        secondaryLabel="New folder"
        onprimary={onadd}
        onsecondary={onnewfolder}
      />{/if}
  {/if}
</section>

<style>
  .browse-page {
    width: 100%;
  }
  .context {
    display: flex;
    flex: 1;
    min-width: 0;
    align-items: center;
  }
  .icon-action {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.16rem;
    width: 2.65rem;
    height: 2.65rem;
    padding: 0;
    border: 1px solid var(--color-border);
    color: var(--color-ink-text);
    background: transparent;
    cursor: pointer;
  }
  .icon-action:hover:not(:disabled) {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }
  .icon-action :global(.file-icon) {
    color: currentcolor;
  }
</style>
