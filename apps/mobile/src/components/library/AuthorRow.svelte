<script lang="ts">
  import { onMount } from "svelte";

  import { getThumbnailUrl } from "../../lib/thumbnail-cache";
  import { BROWSING_LAYOUT_CONTRACT } from "../../lib/types";
  import type { AuthorSummary } from "../../lib/bindings";

  let {
    author,
    libraryId,
    active = false,
    authorIndex,
    tabindex = -1,
    onfocus,
    onkeydown,
    onselect,
    onopenbook,
  }: {
    author: AuthorSummary;
    libraryId: string;
    active?: boolean;
    authorIndex: number;
    tabindex?: number;
    onfocus?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    onselect: () => void;
    onopenbook: (contentHash: string) => void | Promise<void>;
  } = $props();

  const slots = $derived(author.cover_hashes.slice(0, BROWSING_LAYOUT_CONTRACT.author_row_cover_count).map((hash, index) => ({ hash, title: author.cover_titles[index] ?? "Untitled" })));
  const coverCount = $derived(Math.max(1, slots.length));
  let covers = $state<string[]>([]);
  let thumbnailsLoaded = $state(false);
  let cardElement = $state<HTMLElement>();
  let thumbnailsStarted = false;

  async function loadThumbnails() {
    if (thumbnailsStarted) return;
    thumbnailsStarted = true;
    covers = await Promise.all(slots.map(async (book) => (await getThumbnailUrl(libraryId, book.hash)) ?? ""));
    thumbnailsLoaded = true;
  }

  function handleKeydown(event: KeyboardEvent) {
    onkeydown?.(event);
  }

  onMount(() => {
    const element = cardElement;
    if (!element) return;
    if (!("IntersectionObserver" in window)) {
      void loadThumbnails();
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      void loadThumbnails();
    }, { rootMargin: "640px" });
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<article
  class="card"
  class:active
  class:two-covers={coverCount === 2}
  class:three-covers={coverCount >= 3}
  bind:this={cardElement}
>
  <button
    class="author-target"
    type="button"
    data-author-index={authorIndex}
    {tabindex}
    aria-label={`Open ${author.name || "Unknown author"}`}
    onfocus={() => onfocus?.()}
    onkeydown={handleKeydown}
    onclick={() => onselect()}
  ></button>
  <strong>{author.name || "Unknown author"}</strong>
  <span class="covers">
    {#each slots as book, index (book.hash)}
      <button
        class:empty={thumbnailsLoaded && !covers[index]}
        type="button"
        aria-label={`Open ${book.title}`}
        onclick={(event) => {
          event.stopPropagation();
          void onopenbook(book.hash);
        }}
      >
        {#if covers[index]}
          <img src={covers[index]} alt="" loading="lazy" decoding="async" />
        {:else if thumbnailsLoaded}
          <span>{book.title}</span>
        {/if}
      </button>
    {/each}
  </span>
</article>

<style>
  .card {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: var(--author-row-content-gap);
    width: calc(var(--author-row-cover-width) + var(--author-row-horizontal-padding) * 2);
    min-height: var(--author-row-height);
    padding: var(--author-row-horizontal-padding);
    border: 0;
    border-radius: 0;
    background: var(--color-raised-bg);
    box-shadow: 0 0 0 1px #000;
    color: inherit;
    cursor: pointer;
    box-sizing: border-box;
    position: relative;
    -webkit-tap-highlight-color: transparent;
  }
  .card.two-covers { width: calc(var(--author-row-cover-width) * 2 + var(--author-row-cover-gap) + var(--author-row-horizontal-padding) * 2); }
  .card.three-covers { width: calc(var(--author-row-cover-width) * 3 + var(--author-row-cover-gap) * 2 + var(--author-row-horizontal-padding) * 2); }
  .card:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .card.active { background: var(--color-active-highlight); outline: 1px solid var(--color-accent); }
  .author-target { position: absolute; z-index: 1; inset: 0; padding: 0; border: 0; background: transparent; cursor: pointer; }
  .author-target:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  strong {
    display: -webkit-box;
    width: 100%;
    height: var(--book-card-title-block-height);
    overflow: hidden;
    color: var(--color-ink-text);
    font-size: var(--author-row-font-size);
    font-weight: var(--book-card-title-weight);
    line-height: var(--book-card-title-line-height);
    text-align: left;
    overflow-wrap: anywhere;
    pointer-events: none;
    -webkit-box-orient: vertical;
    line-clamp: 2;
    -webkit-line-clamp: 2;
  }
  .covers { display: flex; z-index: 2; gap: var(--author-row-cover-gap); height: var(--author-row-cover-height); pointer-events: none; }
  .covers button {
    display: grid;
    flex: none;
    place-items: end center;
    width: var(--author-row-cover-width);
    height: var(--author-row-cover-height);
    padding: 0;
    overflow: hidden;
    border: 0;
    border-radius: 0;
    background: transparent;
    cursor: pointer;
    box-sizing: border-box;
    pointer-events: auto;
  }
  .covers button.empty { border: 1px solid var(--color-border); }
  .covers button.empty span { display: -webkit-box; max-height: 5.25rem; padding: 14px; overflow: hidden; color: var(--color-subtle-text); font-size: .9375rem; font-weight: 600; line-height: 1.4; text-align: center; overflow-wrap: anywhere; -webkit-box-orient: vertical; line-clamp: 4; -webkit-line-clamp: 4; }
  .covers img { width: auto; max-width: 100%; height: auto; max-height: 100%; border: 1px solid var(--color-border); object-fit: contain; object-position: center bottom; }
  @media (max-width: 700px) {
    .card.three-covers { width: calc(var(--author-row-cover-width) * 2 + var(--author-row-cover-gap) + var(--author-row-horizontal-padding) * 2); }
    .covers button:nth-child(3) { display: none; }
  }
  @media (max-width: 480px) {
    .card.two-covers, .card.three-covers { width: calc(var(--author-row-cover-width) + var(--author-row-horizontal-padding) * 2); }
    .covers button:nth-child(2) { display: none; }
  }
</style>
