<script lang="ts">
  import type { BookSummary, DownloadState } from "../../lib/types";
  import BookCard from "./BookCard.svelte";

  let {
    books,
    emptyMessage = "",
    openingBookId = "",
    downloadStates = {},
    pageSize = 48,
    onopen,
    ondownload,
  }: {
    books: BookSummary[];
    emptyMessage?: string;
    openingBookId?: string;
    downloadStates?: Record<string, DownloadState>;
    pageSize?: number;
    onopen: (book: BookSummary) => void | Promise<void>;
    ondownload: (book: BookSummary) => void | Promise<void>;
  } = $props();

  let visibleCount = $state(48);
  const visibleBooks = $derived(books.slice(0, visibleCount));

  $effect(() => {
    books;
    visibleCount = pageSize;
  });
</script>

{#if books.length}
  <div class="book-grid">
    {#each visibleBooks as book (book.id)}
      <BookCard {book} opening={openingBookId === book.id} downloadState={downloadStates[book.id]} {onopen} {ondownload} />
    {/each}
  </div>
  {#if visibleCount < books.length}
    <button class="load-more" type="button" onclick={() => (visibleCount += pageSize)}>
      Show more ({books.length - visibleCount})
    </button>
  {/if}
{:else if emptyMessage}
  <div class="inline-empty">{emptyMessage}</div>
{/if}

<style>
  .book-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 1.15rem .8rem; }
  .load-more { display: block; min-height: 2.6rem; margin: 1.25rem auto 0; padding: .55rem 1rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: var(--color-accent); background: var(--color-white-surface); font: 700 .76rem var(--font-interface-family); cursor: pointer; }
  .inline-empty { display: grid; min-height: 5rem; place-items: center start; color: var(--color-subtle-text); font-size: .84rem; text-align: left; }
  @media (min-width: 36rem) { .book-grid { grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 1.35rem 1rem; } }
  @media (min-width: 45rem) { .book-grid { grid-template-columns: repeat(auto-fill, minmax(8.5rem, 1fr)); } }
</style>
