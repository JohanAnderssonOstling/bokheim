<script lang="ts">
  import { onMount } from "svelte";

  import { getThumbnailUrl } from "../../lib/thumbnail-cache";
  import { BROWSING_LAYOUT_CONTRACT } from "../../lib/types";
  import type { AuthorSummary } from "../../lib/bindings";

  let {
    author,
    libraryId,
    active = false,
    tabindex = -1,
    onfocus,
    onkeydown,
    onselect,
  }: {
    author: AuthorSummary;
    libraryId: string;
    active?: boolean;
    tabindex?: number;
    onfocus?: () => void;
    onkeydown?: (event: KeyboardEvent) => void;
    onselect: () => void;
  } = $props();

  // One frame per book the author actually has: a second slot for a book that
  // does not exist would draw an absence. The row keeps the full width either
  // way, so the names still line up down the list.
  const slots = $derived(author.cover_hashes.slice(0, BROWSING_LAYOUT_CONTRACT.author_row_cover_count));
  let covers = $state<string[]>([]);
  let rowElement = $state<HTMLElement>();
  let thumbnailsStarted = false;

  async function loadThumbnails() {
    if (thumbnailsStarted) return;
    thumbnailsStarted = true;
    covers = await Promise.all(slots.map(async (hash) => (hash ? ((await getThumbnailUrl(libraryId, hash)) ?? "") : "")));
  }

  onMount(() => {
    const element = rowElement;
    if (!element) return;
    if (!("IntersectionObserver" in window)) {
      void loadThumbnails();
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      void loadThumbnails();
    }, { rootMargin: "320px" });
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<button class:active type="button" bind:this={rowElement} {tabindex} onfocus={() => onfocus?.()} onkeydown={(event) => onkeydown?.(event)} onclick={() => onselect()}>
  <span class="covers" aria-hidden="true">
    {#each slots as _, index}
      <span class="cover" class:empty={!covers[index]}>
        {#if covers[index]}<img src={covers[index]} alt="" loading="lazy" decoding="async" />{/if}
      </span>
    {/each}
  </span>
  <strong>{author.name || "Unknown author"}</strong>
</button>

<style>
  button {
    display: flex;
    align-items: center;
    gap: .75rem;
    width: 100%;
    height: var(--author-row-height);
    padding: 0 .55rem;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    -webkit-tap-highlight-color: transparent;
  }
  button:hover { background: var(--color-raised-bg); }
  button.active { background: var(--color-accent-soft, var(--color-raised-bg)); border-color: var(--color-border); }
  /* Fixed width whatever it holds, so a one-book author's name starts where
     everyone else's does. */
  .covers { display: flex; gap: .3rem; flex: none; width: calc(var(--author-row-cover-width) * 2 + .3rem); }
  .cover {
    display: grid;
    place-items: end center;
    width: var(--author-row-cover-width);
    height: var(--author-row-cover-height);
    overflow: hidden;
    background: var(--color-raised-bg);
    border: 1px solid var(--color-border);
  }
  /* The slot an author with one book — or one thumbnail — leaves empty. */
  .cover.empty { border-style: dashed; background: none; }
  .cover img { width: 100%; height: 100%; object-fit: contain; object-position: center bottom; }
  strong { min-width: 0; font-weight: 500; font-size: .95rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
