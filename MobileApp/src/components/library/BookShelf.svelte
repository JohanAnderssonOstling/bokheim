<script lang="ts">
  import type { BookSummary, DownloadState } from "../../lib/types";
  import BookCard from "./BookCard.svelte";

  let {
    id,
    title,
    books,
    emptyMessage,
    first = false,
    maxItems,
    openingBookId = "",
    downloadStates = {},
    onopen,
    ondownload,
  }: {
    id: string;
    title: string;
    books: BookSummary[];
    emptyMessage: string;
    first?: boolean;
    maxItems?: number;
    openingBookId?: string;
    downloadStates?: Record<string, DownloadState>;
    onopen: (book: BookSummary) => void | Promise<void>;
    ondownload: (book: BookSummary) => void | Promise<void>;
  } = $props();

  const visibleBooks = $derived(maxItems ? books.slice(0, maxItems) : books);

  function scroll(direction: -1 | 1) {
    const track = document.getElementById(id);
    if (!track) return;
    const card = track.querySelector<HTMLElement>(".slide");
    const gap = Number.parseFloat(getComputedStyle(track).columnGap) || 0;
    const step = Math.max((card?.offsetWidth ?? 144) + gap, track.clientWidth * .72);
    track.scrollBy({ left: direction * step, behavior: "smooth" });
  }
</script>

<section class:first class="book-shelf">
  <div class="heading">
    <h2>{title}</h2>
    <div class="actions">
      {#if visibleBooks.length}
        <div class="controls" aria-label={`${title} carousel controls`}>
          <button type="button" aria-label={`Previous ${title}`} onclick={() => scroll(-1)}>‹</button>
          <button type="button" aria-label={`Next ${title}`} onclick={() => scroll(1)}>›</button>
        </div>
      {/if}
    </div>
  </div>

  {#if visibleBooks.length}
    <div class="track" {id} role="region" aria-label={`${title}, scrollable books`}>
      {#each visibleBooks as book (book.id)}
        <div class="slide"><BookCard {book} opening={openingBookId === book.id} downloadState={downloadStates[book.id]} {onopen} {ondownload} /></div>
      {/each}
    </div>
  {:else}
    <div class="inline-empty">{emptyMessage}</div>
  {/if}
</section>

<style>
  .book-shelf { margin-top: 1.8rem; }
  .book-shelf.first { margin-top: 0; }
  .heading { display: flex; align-items: end; justify-content: space-between; gap: 1rem; margin-bottom: .8rem; }
  h2 { margin: 0; font: 500 1.35rem var(--font-heading-family); letter-spacing: -.02em; }
  .actions, .controls { display: flex; align-items: center; }
  .actions { gap: .65rem; }
  .controls { gap: .3rem; }
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  .controls button { display: grid; place-items: center; width: 2.1rem; height: 2.1rem; padding: 0 0 .14rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: var(--color-accent); background: var(--color-white-surface); font-size: 1.35rem; line-height: 1; cursor: pointer; }
  .controls button:hover { background: var(--color-hover-bg); }
  .controls button:active { transform: translateY(1px); }
  .track { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(7.5rem, 42%); gap: .8rem; width: 100%; padding: .1rem .05rem .75rem; overflow-x: auto; overscroll-behavior-inline: contain; scroll-padding-inline: .05rem; scroll-snap-type: inline mandatory; scrollbar-color: var(--color-disabled-text) transparent; scrollbar-width: thin; -webkit-overflow-scrolling: touch; }
  .track::-webkit-scrollbar { height: .42rem; }
  .track::-webkit-scrollbar-track { background: transparent; }
  .track::-webkit-scrollbar-thumb { border-radius: 999px; background: var(--color-disabled-text); }
  .slide { min-width: 0; scroll-snap-align: start; }
  .inline-empty { display: grid; min-height: 5rem; place-items: center start; color: var(--color-subtle-text); font-size: .84rem; text-align: left; }
  @media (min-width: 36rem) { .track { grid-auto-columns: minmax(8.5rem, 30%); gap: 1rem; } }
  @media (min-width: 45rem) { .track { grid-auto-columns: minmax(8.5rem, 10.5rem); } }
  @media (min-width: 70rem) { .track { grid-auto-columns: minmax(9rem, 11rem); } }
</style>
