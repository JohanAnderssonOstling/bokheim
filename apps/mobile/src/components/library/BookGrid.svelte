<script lang="ts">
  import { onMount } from "svelte";
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

  let gridElement = $state<HTMLDivElement>();
  let distributeColumns = $state(false);

  function updateColumnDistribution() {
    if (!gridElement || detailed || window.innerWidth < 720) {
      distributeColumns = false;
      return;
    }
    const style = getComputedStyle(gridElement);
    const coverWidth = Number.parseFloat(style.getPropertyValue("--book-cover-width")) || 200;
    const baseGap = Number.parseFloat(style.getPropertyValue("--book-grid-column-gap")) || 28;
    const columns = Math.max(1, Math.floor((gridElement.clientWidth + baseGap) / (coverWidth + baseGap)));
    distributeColumns = columns > 1 && books.length >= columns;
  }

  $effect(() => {
    books.length;
    detailed;
    queueMicrotask(updateColumnDistribution);
  });

  onMount(() => {
    updateColumnDistribution();
    const observer = new ResizeObserver(updateColumnDistribution);
    if (gridElement) observer.observe(gridElement);
    return () => observer.disconnect();
  });

</script>

{#if books.length}
  <div class={detailed ? "book-grid detailed" : "book-grid"} class:distributed={distributeColumns} bind:this={gridElement}>
    {#each books as book (book.content_hash)}
    <BookCard {book} {libraryId} detailed={detailed} opening={openingBookId === book.content_hash} downloadState={downloadStates[book.content_hash]} {onopen} {ondownload} {onremove} {ontrash} {oncopytofolder} {onmovetofolder} {onremovefromfolder} />
    {/each}
  </div>
{:else if emptyMessage}
  <EmptyPage title={emptyMessage} />
{/if}

<style>
  .book-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 18px 14px; }
  .book-grid.detailed { grid-template-columns: minmax(0, 1fr); gap: 0; }
  .book-grid :global(.book-card) { content-visibility: auto; contain-intrinsic-size: var(--book-cover-width) var(--book-card-height); }
  .book-grid.detailed :global(.book-card) { width: 100%; }
  @media (min-width: 36rem) { .book-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); } }
  @media (min-width: 45rem) {
    .book-grid { grid-template-columns: repeat(auto-fill, var(--book-cover-width)); column-gap: var(--book-grid-column-gap); row-gap: calc(var(--book-grid-row-height) - var(--book-card-height)); }
    .book-grid.distributed { justify-content: space-between; }
    .book-grid.detailed { grid-template-columns: 1fr; row-gap: 0; }
  }
</style>
