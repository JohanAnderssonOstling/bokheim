<script lang="ts">
  import BookShelf from "./BookShelf.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";
  import { BROWSING_CONTENT_CONTRACT } from "../../lib/types";
  import type { BookCardData, DownloadState } from "../../lib/types";
  let { libraryId, inProgress, finished, recentlyAdded, openingBookId, downloadStates, onadd, onopen, ondownload, onremove, ontrash }: {
    libraryId: string; inProgress: BookCardData[]; finished: BookCardData[]; recentlyAdded: BookCardData[]; openingBookId: string;
    downloadStates: Record<string, DownloadState>; onadd: () => void | Promise<void>;
    onopen: (book: BookCardData) => void | Promise<void>; ondownload: (book: BookCardData) => void | Promise<void>; onremove: (book: BookCardData) => void | Promise<void>; ontrash: (book: BookCardData) => void | Promise<void>;
  } = $props();
</script>

{#if !inProgress.length && !finished.length && !recentlyAdded.length}
  <EmptyPage primaryLabel="Add book" onprimary={onadd} />
{:else}
  {#if inProgress.length}<BookShelf {libraryId} id="recently-read-carousel" title="Recently read" books={inProgress} emptyMessage="No books in progress yet." first {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} />{/if}
  {#if finished.length}<BookShelf {libraryId} id="finished-carousel" title="Finished" books={finished} emptyMessage="No finished books yet." first={!inProgress.length} {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} />{/if}
  {#if recentlyAdded.length}<BookShelf {libraryId} id="recently-added-carousel" title="Recently added" books={recentlyAdded} emptyMessage="No books have been added yet." first={!inProgress.length && !finished.length} maxItems={BROWSING_CONTENT_CONTRACT.home_book_limit} {openingBookId} {downloadStates} {onopen} {ondownload} {onremove} {ontrash} />{/if}
{/if}
