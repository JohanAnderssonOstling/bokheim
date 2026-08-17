<script lang="ts">
  import type { BookCardData, DownloadState } from "../../lib/types";
  import BookCard from "./BookCard.svelte";

  let {
    id,
    title,
    books,
    libraryId,
    emptyMessage,
    first = false,
    maxItems,
    openingBookId = "",
    downloadStates = {},
    onopen,
    ondownload,
    onremove,
    ontrash,
  }: {
    id: string;
    title: string;
    books: BookCardData[];
    libraryId: string;
    emptyMessage: string;
    first?: boolean;
    maxItems?: number;
    openingBookId?: string;
    downloadStates?: Record<string, DownloadState>;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
    ontrash: (book: BookCardData) => void | Promise<void>;
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
      {#each visibleBooks as book (book.content_hash)}
        <div class="slide"><BookCard {book} {libraryId} opening={openingBookId === book.content_hash} downloadState={downloadStates[book.content_hash]} {onopen} {ondownload} {onremove} {ontrash} /></div>
      {/each}
    </div>
  {:else}
    <div class="inline-empty">{emptyMessage}</div>
  {/if}
</section>

<style>
  .book-shelf { margin-top: var(--carousel-section-gap); }
  .book-shelf.first { margin-top: 0; }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: var(--carousel-content-gap); margin-bottom: var(--carousel-content-gap); }
  h2 { margin: 0; font: var(--carousel-title-weight) var(--carousel-title-size) var(--font-interface-family); }
  .actions, .controls { display: flex; align-items: center; }
  .actions { gap: 9px; }
  .controls { gap: var(--carousel-control-gap); }
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  .controls button { display: grid; place-items: center; width: var(--carousel-arrow-size); height: var(--carousel-arrow-size); padding: 0 0 2px; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-accent); background: var(--color-raised-bg); font-size: 21px; line-height: 1; cursor: pointer; }
  .controls button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .controls button:active { transform: translateY(1px); }
  .track { display: grid; grid-auto-flow: column; grid-auto-columns: minmax(120px, 42%); gap: 14px; width: 100%; padding: 0 1px 6px; overflow-x: auto; overscroll-behavior-inline: contain; scroll-padding-inline: 1px; scroll-snap-type: inline mandatory; scrollbar-color: var(--color-border) transparent; scrollbar-width: thin; -webkit-overflow-scrolling: touch; }
  .track::-webkit-scrollbar { height: 7px; }
  .track::-webkit-scrollbar-track { background: transparent; }
  .track::-webkit-scrollbar-thumb { border-radius: 0; background: var(--color-border); }
  .slide { min-width: 0; scroll-snap-align: start; }
  .inline-empty { display: grid; min-height: 80px; place-items: center start; color: var(--color-subtle-text); font-size: .8125rem; text-align: left; }
  @media (min-width: 36rem) { .track { grid-auto-columns: minmax(136px, 30%); gap: 18px; } }
  @media (min-width: 45rem) { .track { grid-auto-columns: var(--book-cover-width); column-gap: var(--book-grid-column-gap); } }
</style>
