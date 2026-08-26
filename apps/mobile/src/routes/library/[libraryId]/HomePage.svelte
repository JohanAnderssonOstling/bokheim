<script lang="ts">
  import BookShelf from "../../../components/shared/books/BookShelf.svelte";
  import EmptyPage from "../../../components/shell/EmptyPage.svelte";
  import { BROWSING_CONTENT_CONTRACT } from "../../../lib/shared/types";
  import type { BookCardData, DownloadState } from "../../../lib/shared/types";
  let {
    libraryId,
    continueReading,
    inProgress,
    finished,
    recentlyAdded,
    openingBookId,
    downloadStates,
    onadd,
    onopen,
    ondownload,
    onremove,
    ontrash,
  }: {
    libraryId: string;
    continueReading: BookCardData[];
    inProgress: BookCardData[];
    finished: BookCardData[];
    recentlyAdded: BookCardData[];
    openingBookId: string;
    downloadStates: Record<string, DownloadState>;
    onadd: () => void | Promise<void>;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
    ontrash: (book: BookCardData) => void | Promise<void>;
  } = $props();
</script>

{#if !continueReading.length && !inProgress.length && !finished.length && !recentlyAdded.length}
  <EmptyPage
    primaryLabel="Add book"
    onprimary={onadd}
  />
{:else}
  {#if continueReading.length}<BookShelf
      {libraryId}
      id="continue-reading-carousel"
      title="Furthest along"
      books={continueReading}
      emptyMessage="No books in progress yet."
      first
      {openingBookId}
      {downloadStates}
      {onopen}
      {ondownload}
      {onremove}
      {ontrash}
    />{/if}
  {#if inProgress.length}<BookShelf
      {libraryId}
      id="recently-read-carousel"
      title="Recently read"
      books={inProgress}
      emptyMessage="No books in progress yet."
      first={!continueReading.length}
      {openingBookId}
      {downloadStates}
      {onopen}
      {ondownload}
      {onremove}
      {ontrash}
    />{/if}
  {#if finished.length}<BookShelf
      {libraryId}
      id="finished-carousel"
      title="Finished"
      books={finished}
      emptyMessage="No finished books yet."
      first={!continueReading.length && !inProgress.length}
      {openingBookId}
      {downloadStates}
      {onopen}
      {ondownload}
      {onremove}
      {ontrash}
    />{/if}
  {#if recentlyAdded.length}<BookShelf
      {libraryId}
      id="recently-added-carousel"
      title="Recently added"
      books={recentlyAdded}
      emptyMessage="No books have been added yet."
      first={!continueReading.length && !inProgress.length && !finished.length}
      maxItems={BROWSING_CONTENT_CONTRACT.home_book_limit}
      {openingBookId}
      {downloadStates}
      {onopen}
      {ondownload}
      {onremove}
      {ontrash}
    />{/if}
{/if}
