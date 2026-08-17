<script lang="ts">
  import type { Snippet } from "svelte";
  import BookGrid from "./BookGrid.svelte";
  import BrowseNavigationGrid from "./BrowseNavigationGrid.svelte";
  import BrowseFilterBar from "./BrowseFilterBar.svelte";
  import FolderNavigationCard from "./FolderNavigationCard.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import MetadataReviewPanel from "./MetadataReviewPanel.svelte";
  import { BROWSING_CONTENT_CONTRACT, type BookCardData, type DownloadState, type LibraryDirectory, type LibraryFileTypeFilter } from "../../lib/types";
  let { libraryId, subjects, books, subjectPath, fileType, loading, query = $bindable(), detailed, openingBookId, downloadStates, searchResults, onsearch, onfiletype, onopensubject, onimported, onopen, ondownload, onremove, ontrash }: {
    libraryId: string; subjects: LibraryDirectory[]; books: BookCardData[]; subjectPath: { id: string; name: string }[]; loading: boolean; query: string; detailed: boolean;
    fileType: LibraryFileTypeFilter; openingBookId: string; downloadStates: Record<string, DownloadState>; searchResults: Snippet; onsearch: () => void; onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>; onimported: () => void | Promise<void>;
    onopensubject: (subject: LibraryDirectory) => void | Promise<void>; onopen: (book: BookCardData) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrash: (book: BookCardData) => void | Promise<void>;
  } = $props();
  const placeholder = $derived(subjectPath.length ? `Search books or authors in ${subjectPath.at(-1)?.name} and subsubjects` : "Search books or authors in all subjects");
  const selectedTypeLabel = $derived(BROWSING_CONTENT_CONTRACT.file_type_filters.find((option) => option.value === fileType)?.label ?? "books");
</script>
<section class="browse-page" aria-label="Subjects">
  <BrowseFilterBar bind:query {placeholder} {fileType} {onsearch} {onfiletype} />
  {#if query.trim()}{@render searchResults()}
  {:else if loading}<div class="state" aria-live="polite">Loading subjects…</div>
  {:else}
    {#if subjects.length}<BrowseNavigationGrid>{#each subjects as subject}<FolderNavigationCard kind="subject" flat name={subject.name} count={subject.book_count} onclick={() => onopensubject(subject)} />{/each}</BrowseNavigationGrid>{/if}
    {#if subjectPath.at(-1)?.id === "__uncategorized__"}<MetadataReviewPanel {libraryId} onchanged={onimported} />{/if}
    <BookGrid {libraryId} {books} {detailed} emptyMessage="" {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} />
    {#if !subjects.length && !books.length}<EmptyPage title={fileType === "all" ? "No subjects" : `No ${selectedTypeLabel.toLowerCase()} here`} description={fileType === "all" ? "No books have subject information here yet." : `This subject and its subsubjects contain no ${selectedTypeLabel.toLowerCase()}.`} />{/if}
  {/if}
</section>
<style>
  .browse-page { width: 100%; }
  .state { display: grid; min-height: 10rem; place-content: center; color: var(--color-subtle-text); }
</style>
