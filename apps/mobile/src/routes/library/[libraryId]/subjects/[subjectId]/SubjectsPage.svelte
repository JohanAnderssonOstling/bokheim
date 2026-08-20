<script lang="ts">
  import type { Snippet } from "svelte";
  import BookGrid from "../../../../../components/shared/books/BookGrid.svelte";
  import BrowseNavigationGrid from "../../../../../components/shared/browse/BrowseNavigationGrid.svelte";
  import BrowseFilterBar from "../../../../../components/shared/browse/BrowseFilterBar.svelte";
  import FolderNavigationCard from "../../../../../components/shared/browse/FolderNavigationCard.svelte";
  import EmptyPage from "../../../../../components/shell/EmptyPage.svelte";
  import StickyTopbar from "../../../../../components/shared/StickyTopbar.svelte";
  import FolderBreadcrumbs from "../../../../../components/shared/browse/FolderBreadcrumbs.svelte";
  import StatusState from "../../../../../components/shared/StatusState.svelte";
  import MetadataReviewPanel from "./MetadataReviewPanel.svelte";
  import {
    BROWSING_CONTENT_CONTRACT,
    type BookCardData,
    type DownloadState,
    type LibraryDirectory,
    type LibraryFileTypeFilter,
  } from "../../../../../lib/shared/types";
  import type { LibraryPageLevel } from "../../../../../lib/library/page-types";
  let {
    libraryId,
    subjects,
    books,
    subjectPath,
    fileType,
    loading,
    query = $bindable(),
    detailed,
    openingBookId,
    downloadStates,
    searchResults,
    onsearch,
    onquerychange,
    onfiletype,
    onopensubject,
    onimported,
    onopen,
    ondownload,
    onremove,
    ontrash,
    onsubject,
  }: {
    libraryId: string;
    subjects: LibraryDirectory[];
    books: BookCardData[];
    subjectPath: LibraryPageLevel[];
    loading: boolean;
    query: string;
    detailed: boolean;
    fileType: LibraryFileTypeFilter;
    openingBookId: string;
    downloadStates: Record<string, DownloadState>;
    searchResults: Snippet;
    onsearch: () => void;
    onquerychange: (value: string) => void;
    onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>;
    onimported: () => void | Promise<void>;
    onopensubject: (subject: LibraryDirectory) => void | Promise<void>;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
    ontrash: (book: BookCardData) => void | Promise<void>;
    onsubject: (level: number) => void | Promise<void>;
  } = $props();
  const placeholder = $derived(
    subjectPath.length
      ? `Search books or authors in ${subjectPath.at(-1)?.name} and subsubjects`
      : "Search books or authors in all subjects",
  );
  const selectedTypeLabel = $derived(
    BROWSING_CONTENT_CONTRACT.file_type_filters.find(
      (option) => option.value === fileType,
    )?.label ?? "books",
  );
</script>

<section
  class="browse-page"
  aria-label="Subjects"
>
  <StickyTopbar>
    <FolderBreadcrumbs
      levels={subjectPath}
      onselect={onsubject}
    />
  </StickyTopbar>
  <BrowseFilterBar
    bind:query
    {placeholder}
    {fileType}
    {onsearch}
    {onquerychange}
    {onfiletype}
  />
  {#if query.trim()}{@render searchResults()}
  {:else if loading}<StatusState message="Loading subjects…" />
  {:else}
    {#if subjects.length}<BrowseNavigationGrid
        >{#each subjects as subject}<FolderNavigationCard
            kind="subject"
            flat
            name={subject.name}
            count={subject.book_count}
            onclick={() => onopensubject(subject)}
          />{/each}</BrowseNavigationGrid
      >{/if}
    {#if subjectPath.at(-1)?.id === "__uncategorized__"}<MetadataReviewPanel
        {libraryId}
        onchanged={onimported}
      />{/if}
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
    />
    {#if !subjects.length && !books.length}<EmptyPage
        title={fileType === "all"
          ? "No subjects"
          : `No ${selectedTypeLabel.toLowerCase()} here`}
        description={fileType === "all"
          ? "No books have subject information here yet."
          : `This subject and its subsubjects contain no ${selectedTypeLabel.toLowerCase()}.`}
      />{/if}
  {/if}
</section>

<style>
  .browse-page {
    width: 100%;
  }
</style>
