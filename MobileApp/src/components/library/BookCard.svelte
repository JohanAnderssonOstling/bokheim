<script lang="ts">
  import { onMount } from "svelte";

  import { getThumbnailUrl } from "../../lib/thumbnail-cache";
  import { isDownloadActive, isDownloaded } from "../../lib/download-state";
  import { isWebApp } from "../../lib/platform";
  import { BOOK_ACTIONS_CONTRACT, type BookCardData, type DownloadState } from "../../lib/types";
  import { managedMenu } from "../../lib/keyboard-navigation";

  let {
    book,
    libraryId,
    opening = false,
    detailed = false,
    downloadState,
    onopen,
    ondownload,
    onremove,
  }: {
    book: BookCardData;
    libraryId: string;
    opening?: boolean;
    detailed?: boolean;
    downloadState?: DownloadState;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
  } = $props();

  let coverUrl = $state("");
  let coverElement = $state<HTMLElement>();
  let thumbnailStarted = false;
  const title = $derived(book.title || "Untitled");
  const progress = $derived(Math.min(100, Math.max(0, book.progress ?? 0)));
  const backendDownloadState = $derived(book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded");
  const downloaded = $derived(isDownloaded(downloadState ?? backendDownloadState));
  const downloading = $derived(isDownloadActive(downloadState ?? backendDownloadState));
  const description = $derived(() => {
    if (book.description.trim()) return book.description.trim();
    return "No description available.";
  });

  function closeMenu(event: MouseEvent) {
    (event.currentTarget as HTMLElement).closest("details")?.removeAttribute("open");
  }

  async function loadThumbnail() {
    if (thumbnailStarted) return;
    thumbnailStarted = true;
    const url = await getThumbnailUrl(libraryId, book.content_hash);
    if (url) coverUrl = url;
  }

  onMount(() => {
    const element = coverElement;
    if (!element) return;
    if (!("IntersectionObserver" in window)) {
      void loadThumbnail();
      return;
    }

    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      void loadThumbnail();
    }, { rootMargin: "320px" });
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<div class="book-card" class:detailed={detailed}>
  <button class="book-open" class:detailed={detailed} type="button" disabled={opening} aria-label={`Read ${title}`} onclick={() => onopen(book)}>
    {#if detailed}
      <span class="book-details">
        <span class="book-meta">
          <strong>{title}</strong>
          <span>{book.author || "Unknown author"}</span>
        </span>
        <span class="book-description">{description}</span>
      </span>
    {:else}
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
      </span>
    {/if}
    {#if (book.progress ?? 0) > 0}
      <span class="progress-track" aria-label={`${Math.round(progress)}% read`}>
        <i style={`width: ${progress}%`}></i>
      </span>
    {/if}
    {#if opening}<span class="opening">Opening…</span>{/if}
  </button>
  <details class="book-actions" use:managedMenu>
    <summary aria-label={`${BOOK_ACTIONS_CONTRACT.menu_label} for ${title}`} title={BOOK_ACTIONS_CONTRACT.menu_label}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="5" r="1.25"></circle><circle cx="12" cy="12" r="1.25"></circle><circle cx="12" cy="19" r="1.25"></circle></svg>
    </summary>
    <div class="book-menu" role="menu">
      {#if downloaded || isWebApp}<button type="button" role="menuitem" onclick={(event) => { closeMenu(event); return onopen(book); }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5.5h6.5A3.5 3.5 0 0 1 14 9v10.5H7.5A3.5 3.5 0 0 0 4 23zM20 5.5h-2"></path></svg><span>{BOOK_ACTIONS_CONTRACT.open_label}</span></button>{/if}
      {#if !isWebApp && downloaded}
        <button class="danger" type="button" role="menuitem" onclick={(event) => { closeMenu(event); return onremove(book); }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M9 7V4h6v3m-9 0 1 14h10l1-14M10 11v6m4-6v6"></path></svg><span>{BOOK_ACTIONS_CONTRACT.remove_local_copy_label}</span></button>
      {:else if !isWebApp}
        <button type="button" role="menuitem" disabled={downloading} onclick={(event) => { closeMenu(event); return ondownload(book); }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M5 21h14"></path></svg><span>{downloading ? BOOK_ACTIONS_CONTRACT.download_in_progress_label : BOOK_ACTIONS_CONTRACT.download_label}</span></button>
      {/if}
    </div>
  </details>
</div>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }
  .book-card { position: relative; display: grid; width: 100%; min-width: 0; color: inherit; }
  .book-card.detailed { width: 100%; }
  .book-open { display: grid; width: 100%; min-width: 0; padding: 0; border: 0; color: inherit; text-align: left; background: transparent; cursor: pointer; }
  .book-open:active { transform: scale(.985); }
  .book-open:disabled { opacity: .58; }
  .book-open.detailed { display: grid; grid-template-columns: minmax(0, 1.45fr) minmax(0, 1fr); gap: .85rem; min-height: 6.5rem; padding: .75rem .85rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-md); background: var(--color-card-bg); }
  .book-details { display: grid; grid-template-columns: minmax(0, 1fr); gap: .35rem; min-width: 0; align-content: start; }
  .book-meta { display: grid; gap: .18rem; min-width: 0; }
  .book-meta strong, .book-meta span { overflow: hidden; text-overflow: ellipsis; }
  .book-meta strong { display: -webkit-box; min-height: 2em; line-height: 1.15; font-size: .9rem; font-weight: 750; white-space: normal; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2; }
  .book-meta span { color: var(--color-subtle-text); font-size: .7rem; }
  .book-description { display: -webkit-box; min-height: 1.9em; -webkit-line-clamp: 4; line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; color: var(--color-subtle-text); font-size: .74rem; line-height: 1.32; }
  .book-cover { position: relative; display: grid; place-items: center; width: 100%; aspect-ratio: .69; overflow: hidden; border: 1px solid color-mix(in srgb, var(--color-border-color) 55%, transparent); border-radius: var(--radius-sm); color: var(--color-subtle-text); background: var(--color-app-bg); box-shadow: 0 .35rem .9rem color-mix(in srgb, var(--color-box-shadow) 40%, transparent); }
  .book-cover img { width: 100%; height: 100%; object-fit: cover; }
  .book-cover > svg { width: 2rem; height: 2rem; }
  .card-copy { display: grid; gap: .18rem; min-width: 0; padding: .55rem .15rem 0; }
  .card-copy strong, .card-copy > span:not(.progress-track) { overflow: hidden; text-overflow: ellipsis; }
  .card-copy strong { display: -webkit-box; min-height: 2.3em; font-size: .84rem; font-weight: 720; line-height: 1.15; white-space: normal; line-clamp: 2; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
  .card-copy > span:not(.progress-track) { white-space: nowrap; }
  .card-copy > span:not(.progress-track) { color: var(--color-subtle-text); font-size: .69rem; }
  .progress-track { display: block; width: 70%; height: .2rem; margin-top: .3rem; overflow: hidden; border-radius: 99px; background: var(--color-border-color); }
  .progress-track i { display: block; height: 100%; border-radius: inherit; background: var(--color-accent); }
  .opening { position: absolute; inset: auto .45rem 3.8rem; padding: .35rem; border-radius: var(--radius-sm); color: var(--color-white-surface); background: color-mix(in srgb, var(--color-ink-text) 78%, transparent); text-align: center; font-size: .68rem; font-weight: 750; }
  .book-actions { position: absolute; z-index: 4; top: .45rem; right: .45rem; }
  .book-actions summary { display: grid; place-items: center; width: 2.35rem; height: 2.35rem; padding: 0; border: 1px solid color-mix(in srgb, var(--color-border-color) 70%, transparent); border-radius: 50%; color: var(--color-ink-text); background: color-mix(in srgb, var(--color-card-bg) 92%, transparent); box-shadow: 0 .2rem .7rem color-mix(in srgb, var(--color-box-shadow) 40%, transparent); cursor: pointer; list-style: none; backdrop-filter: blur(10px); }
  .book-actions summary::-webkit-details-marker { display: none; }
  .book-actions summary svg { width: 1.15rem; height: 1.15rem; fill: currentColor; stroke: none; }
  .book-actions[open] summary { color: var(--color-white-surface); background: var(--color-accent); }
  .book-menu { position: absolute; top: calc(100% + .35rem); right: 0; display: grid; width: max-content; min-width: 11.5rem; padding: .35rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); background: var(--color-card-bg); box-shadow: 0 .8rem 2rem var(--color-box-shadow); }
  .book-menu button { display: grid; grid-template-columns: 1.1rem minmax(0, 1fr); align-items: center; gap: .55rem; min-height: 2.45rem; padding: .55rem .7rem; border: 0; border-radius: calc(var(--radius-sm) - .15rem); color: var(--color-ink-text); background: transparent; text-align: left; font-size: .76rem; font-weight: 650; cursor: pointer; }
  .book-menu button svg { width: 1.05rem; height: 1.05rem; }
  .book-menu button:hover { background: var(--color-hover-bg); }
  .book-menu button:disabled { color: var(--color-disabled-text); cursor: default; }
  .book-menu button.danger { color: var(--color-danger-text); }
  @media (min-width: 45rem) {
    .book-card { width: var(--book-cover-width); height: var(--book-card-height); }
    .book-card.detailed { width: 100%; height: auto; }
    .book-open { width: var(--book-cover-width); }
    .book-open.detailed { width: 100%; min-height: 5.8rem; }
    .book-cover { width: var(--book-cover-width); height: var(--book-cover-height); aspect-ratio: auto; border-radius: 8px; }
    .card-copy strong { font-size: 17px; }
    .card-copy > span:not(.progress-track) { font-size: 15px; }
    .book-meta strong { font-size: 1rem; }
    .book-meta span { font-size: .78rem; }
    .book-description { font-size: .82rem; }
  }
</style>
