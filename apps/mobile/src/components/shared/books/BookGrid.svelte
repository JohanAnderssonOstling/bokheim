<script lang="ts">
  import type { BookCardData, DownloadState } from "../../../lib/shared/types";
  import BookCard from "./BookCard.svelte";
  import BookGridLayout from "./BookGridLayout.svelte";
  import EmptyPage from "../../shell/EmptyPage.svelte";

  let {
    books,
    libraryId,
    detailed = false,
    emptyMessage = "",
    openingBookId = "",
    downloadStates = {},
    onopen,
    ondownload,
    onremove,
    ontrash,
    oncopytofolder,
    onmovetofolder,
    onremovefromfolder,
  }: {
    books: BookCardData[];
    libraryId: string;
    detailed?: boolean;
    emptyMessage?: string;
    openingBookId?: string;
    downloadStates?: Record<string, DownloadState>;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
    ontrash: (book: BookCardData) => void | Promise<void>;
    oncopytofolder?: (book: BookCardData) => void | Promise<void>;
    onmovetofolder?: (book: BookCardData) => void | Promise<void>;
    onremovefromfolder?: (book: BookCardData) => void | Promise<void>;
  } = $props();
</script>

{#if books.length}
  <BookGridLayout {detailed}>
    {#each books as book (book.content_hash)}
      <BookCard
        {book}
        {libraryId}
        {detailed}
        opening={openingBookId === book.content_hash}
        downloadState={downloadStates[book.content_hash]}
        {onopen}
        {ondownload}
        {onremove}
        {ontrash}
        {oncopytofolder}
        {onmovetofolder}
        {onremovefromfolder}
      />
    {/each}
  </BookGridLayout>
{:else if emptyMessage}
  <EmptyPage title={emptyMessage} />
{/if}
