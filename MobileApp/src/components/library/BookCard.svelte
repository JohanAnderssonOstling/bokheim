<script lang="ts">
  import { onMount } from "svelte";

  import { getThumbnailUrl } from "../../lib/thumbnail-cache";
  import { downloadProgress, isDownloadActive, isDownloaded } from "../../lib/download-state";
  import type { BookSummary, DownloadState } from "../../lib/types";
  import DownloadIndicator from "./DownloadIndicator.svelte";

  let {
    book,
    opening = false,
    downloadState,
    onopen,
    ondownload,
  }: {
    book: BookSummary;
    opening?: boolean;
    downloadState?: DownloadState;
    onopen: (book: BookSummary) => void | Promise<void>;
    ondownload: (book: BookSummary) => void | Promise<void>;
  } = $props();

  let coverUrl = $state("");
  let coverElement: HTMLElement;
  let thumbnailStarted = false;
  const title = $derived(book.title || "Untitled");
  const progress = $derived(Math.min(100, Math.max(0, book.read_progress)));

  async function loadThumbnail() {
    if (thumbnailStarted) return;
    thumbnailStarted = true;
    const url = await getThumbnailUrl(book.id);
    if (url) coverUrl = url;
  }

  onMount(() => {
    if (!("IntersectionObserver" in window)) {
      void loadThumbnail();
      return;
    }

    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      void loadThumbnail();
    }, { rootMargin: "320px" });
    observer.observe(coverElement);
    return () => observer.disconnect();
  });
</script>

<div class="book-card">
  <button class="book-open" type="button" disabled={opening} aria-label={`Read ${title}`} onclick={() => onopen(book)}>
    <span class="book-cover" aria-hidden="true" bind:this={coverElement}>
      {#if coverUrl}
        <img src={coverUrl} alt="" loading="lazy" decoding="async" />
      {:else}
        <svg viewBox="0 0 24 24"><path d="M5 3.5h12a2 2 0 0 1 2 2V21H7a2 2 0 0 1-2-2.5zm0 15A2.5 2.5 0 0 1 7.5 16H19"></path></svg>
      {/if}
    </span>
    <span class="card-copy">
      <strong>{title}</strong>
      <span>{book.author || "Unknown author"}</span>
      {#if book.read_progress > 0}
        <span class="progress-track" aria-label={`${Math.round(progress)}% read`}>
          <i style={`width: ${progress}%`}></i>
        </span>
      {/if}
    </span>
    {#if opening}<span class="opening">Opening…</span>{/if}
  </button>
  {#if downloadState !== undefined}
    <div class="download-action">
      <DownloadIndicator
        progress={downloadProgress(downloadState)}
        active={isDownloadActive(downloadState)}
        complete={isDownloaded(downloadState)}
        label={isDownloaded(downloadState) ? `${title} downloaded` : `Download ${title}`}
        onclick={(event) => { event.stopPropagation(); return ondownload(book); }}
      />
    </div>
  {/if}
</div>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }
  .book-card { position: relative; display: grid; width: 100%; min-width: 0; color: inherit; }
  .book-open { display: grid; width: 100%; min-width: 0; padding: 0; border: 0; color: inherit; text-align: left; background: transparent; cursor: pointer; }
  .book-open:active { transform: scale(.985); }
  .book-open:disabled { opacity: .58; }
  .book-cover { position: relative; display: grid; place-items: center; width: 100%; aspect-ratio: .69; overflow: hidden; border: 1px solid color-mix(in srgb, var(--color-border-color) 55%, transparent); border-radius: var(--radius-sm); color: var(--color-subtle-text); background: var(--color-app-bg); box-shadow: 0 .35rem .9rem color-mix(in srgb, var(--color-box-shadow) 40%, transparent); }
  .book-cover img { width: 100%; height: 100%; object-fit: cover; }
  .book-cover > svg { width: 2rem; height: 2rem; }
  .card-copy { display: grid; gap: .18rem; min-width: 0; padding: .55rem .15rem 0; }
  .card-copy strong, .card-copy > span:not(.progress-track) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .card-copy strong { font-size: .84rem; font-weight: 720; }
  .card-copy > span:not(.progress-track) { color: var(--color-subtle-text); font-size: .69rem; }
  .progress-track { display: block; width: 70%; height: .2rem; margin-top: .3rem; overflow: hidden; border-radius: 99px; background: var(--color-border-color); }
  .progress-track i { display: block; height: 100%; border-radius: inherit; background: var(--color-accent); }
  .opening { position: absolute; inset: auto .45rem 3.8rem; padding: .35rem; border-radius: var(--radius-sm); color: var(--color-white-surface); background: color-mix(in srgb, var(--color-ink-text) 78%, transparent); text-align: center; font-size: .68rem; font-weight: 750; }
  .download-action { position: absolute; top: .45rem; right: .45rem; }
</style>
