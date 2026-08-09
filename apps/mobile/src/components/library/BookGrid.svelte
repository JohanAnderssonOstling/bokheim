<script lang="ts">
  import type { BookCardData, DownloadState } from "../../lib/types";
  import BookCard from "./BookCard.svelte";
  import EmptyPage from "../shell/EmptyPage.svelte";

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
  } = $props();

</script>

{#if books.length}
  <div class={detailed ? "book-grid detailed" : "book-grid"}>
    {#each books as book (book.content_hash)}
    <BookCard {book} {libraryId} detailed={detailed} opening={openingBookId === book.content_hash} downloadState={downloadStates[book.content_hash]} {onopen} {ondownload} {onremove} />
    {/each}
  </div>
{:else if emptyMessage}
  <EmptyPage title={emptyMessage} />
{/if}

<style>
  .book-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1.15rem .8rem; }
  .book-grid.detailed { grid-template-columns: minmax(0, 1fr); gap: .8rem; }
  .book-grid :global(.book-card) { content-visibility: auto; contain-intrinsic-size: var(--book-cover-width) var(--book-card-height); }
  .book-grid.detailed :global(.book-card) { width: 100%; }
  @media (min-width: 36rem) { .book-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 1.35rem 1rem; } }
  @media (min-width: 45rem) {
    .book-grid { grid-template-columns: repeat(auto-fill, var(--book-cover-width)); column-gap: var(--book-grid-column-gap); row-gap: calc(var(--book-grid-row-height) - var(--book-card-height)); }
    .book-grid.detailed { grid-template-columns: 1fr; row-gap: .9rem; }
  }
</style>
