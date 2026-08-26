<script lang="ts">
  import { onMount, tick } from "svelte";

  import { getThumbnailUrl } from "../../../lib/library/thumbnail-cache";
  import { isDownloadActive, isDownloaded } from "../../../lib/library/download-state";
  import { isWebApp } from "../../../lib/app/platform";
  import {
    BOOK_ACTIONS_CONTRACT,
    type BookCardData,
    type DownloadState,
  } from "../../../lib/shared/types";
  import { managedMenu } from "../../../lib/app/keyboard-navigation";
  import BookCoverButton from "../BookCoverButton.svelte";
  import FileIcon from "../FileIcon.svelte";
  import MenuItemButton from "../buttons/MenuItemButton.svelte";
  import SecondaryButton from "../buttons/SecondaryButton.svelte";
  import bookOpen from "@icons/phosphor/book-open.svg";
  import folderOpen from "@icons/phosphor/folder-open.svg";
  import folder from "@icons/phosphor/folder.svg";
  import arrowRight from "@icons/phosphor/arrow-right.svg";
  import trash from "@icons/phosphor/trash.svg";
  import x from "@icons/phosphor/x.svg";
  import download from "@icons/shared/download.svg";

  let {
    book,
    libraryId,
    opening = false,
    detailed = false,
    downloadState,
    onopen,
    ondownload,
    onremove,
    ontrash,
    oncopytofolder,
    onmovetofolder,
    onremovefromfolder,
  }: {
    book: BookCardData;
    libraryId: string;
    opening?: boolean;
    detailed?: boolean;
    downloadState?: DownloadState;
    onopen: (book: BookCardData) => void | Promise<void>;
    ondownload: (book: BookCardData) => void | Promise<void>;
    onremove: (book: BookCardData) => void | Promise<void>;
    ontrash: (book: BookCardData) => void | Promise<void>;
    oncopytofolder?: (book: BookCardData) => void | Promise<void>;
    onmovetofolder?: (book: BookCardData) => void | Promise<void>;
    onremovefromfolder?: (book: BookCardData) => void | Promise<void>;
  } = $props();

  let coverUrl = $state("");
  let thumbnailResolved = $state(false);
  let coverElement = $state<HTMLElement>();
  let actions = $state<HTMLDetailsElement>();
  let menuPosition = $state({ x: 0, y: 0 });
  let thumbnailStarted = false;
  let longPressTimer: ReturnType<typeof setTimeout> | undefined;
  let longPressOrigin: { x: number; y: number } | undefined;
  let suppressOpenUntil = 0;
  const title = $derived(book.title || "Untitled");
  const backendDownloadState = $derived(
    book.downloaded ? "Downloaded" : book.download_requested ? "Queued" : "NotDownloaded",
  );
  const downloaded = $derived(isDownloaded(downloadState ?? backendDownloadState));
  const downloading = $derived(isDownloadActive(downloadState ?? backendDownloadState));
  const description = $derived(() => {
    if (book.description.trim()) return book.description.trim();
    return "No description available.";
  });

  function closeMenu(event: MouseEvent) {
    (event.currentTarget as HTMLElement).closest("details")?.removeAttribute("open");
  }

  async function showActions(x: number, y: number) {
    suppressOpenUntil = Date.now() + 800;
    if (!actions) return;
    menuPosition = { x, y };
    actions.open = true;
    await tick();
    const menu = actions.querySelector<HTMLElement>(".book-menu");
    if (!menu || !actions.open) return;
    const padding = 8;
    menuPosition = {
      x: Math.max(padding, Math.min(x, window.innerWidth - menu.offsetWidth - padding)),
      y: Math.max(padding, Math.min(y, window.innerHeight - menu.offsetHeight - padding)),
    };
  }

  function cancelLongPress() {
    if (longPressTimer) clearTimeout(longPressTimer);
    longPressTimer = undefined;
    longPressOrigin = undefined;
  }

  function startLongPress(event: PointerEvent) {
    if (event.pointerType === "mouse" || event.button !== 0) return;
    cancelLongPress();
    longPressOrigin = { x: event.clientX, y: event.clientY };
    longPressTimer = setTimeout(() => {
      longPressTimer = undefined;
      const origin = longPressOrigin;
      if (!origin) return;
      navigator.vibrate?.(10);
      void showActions(origin.x, origin.y);
    }, 500);
  }

  function trackLongPress(event: PointerEvent) {
    if (!longPressOrigin) return;
    if (
      Math.hypot(event.clientX - longPressOrigin.x, event.clientY - longPressOrigin.y) > 8
    )
      cancelLongPress();
  }

  function openContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    cancelLongPress();
    void showActions(event.clientX, event.clientY);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
    event.preventDefault();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    void showActions(rect.left + rect.width / 2, rect.top + rect.height / 2);
  }

  function openBook(event: MouseEvent) {
    if (Date.now() < suppressOpenUntil) {
      suppressOpenUntil = 0;
      event.preventDefault();
      return;
    }
    return onopen(book);
  }

  function contextActions(node: HTMLElement) {
    node.addEventListener("pointerdown", startLongPress);
    node.addEventListener("pointerup", cancelLongPress);
    node.addEventListener("pointercancel", cancelLongPress);
    node.addEventListener("pointermove", trackLongPress);
    node.addEventListener("contextmenu", openContextMenu);
    node.addEventListener("keydown", handleKeydown);
    return {
      destroy() {
        cancelLongPress();
        node.removeEventListener("pointerdown", startLongPress);
        node.removeEventListener("pointerup", cancelLongPress);
        node.removeEventListener("pointercancel", cancelLongPress);
        node.removeEventListener("pointermove", trackLongPress);
        node.removeEventListener("contextmenu", openContextMenu);
        node.removeEventListener("keydown", handleKeydown);
      },
    };
  }

  async function loadThumbnail() {
    if (thumbnailStarted) return;
    thumbnailStarted = true;
    try {
      const url = await getThumbnailUrl(libraryId, book.content_hash);
      if (url) coverUrl = url;
    } finally {
      thumbnailResolved = true;
    }
  }

  onMount(() => {
    const element = coverElement;
    if (!element) return;
    if (!("IntersectionObserver" in window)) {
      void loadThumbnail();
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        void loadThumbnail();
      },
      { rootMargin: "320px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  });
</script>

<div
  class="book-card"
  class:detailed
  use:contextActions
>
  <BookCoverButton
    className={detailed ? "cover-button detailed" : "cover-button"}
    disabled={opening}
    ariaLabel={`Open details for ${title}`}
    onclick={openBook}
  >
    <span
      class="book-cover"
      class:detailed
      class:missing={thumbnailResolved && !coverUrl}
      aria-hidden="true"
      bind:this={coverElement}
    >
      {#if coverUrl}
        <img
          src={coverUrl}
          alt=""
          loading="lazy"
          decoding="async"
        />
      {:else if thumbnailResolved}
        <FileIcon source={bookOpen} />
        <small>{book.audiobook ? "Audiobook" : "No cover"}</small>
      {/if}
    </span>
    {#if opening}<span class="opening">Opening…</span>{/if}
  </BookCoverButton>
  <span class={detailed ? "book-details" : "card-copy"}>
    <span class="book-meta">
      <strong>{title}</strong>
      <span>{book.author || "Unknown author"}</span>
    </span>
    {#if detailed}<span class="book-description">{description}</span>{/if}
  </span>
  {#if detailed}<div class="wide-open">
      <SecondaryButton
        disabled={opening}
        onclick={openBook}>{BOOK_ACTIONS_CONTRACT.open_label}</SecondaryButton
      >
    </div>{/if}
  <details
    class="book-actions"
    style={`left: ${menuPosition.x}px; top: ${menuPosition.y}px;`}
    bind:this={actions}
    use:managedMenu
  >
    <summary
      aria-label={`${BOOK_ACTIONS_CONTRACT.menu_label} for ${title}`}
      tabindex="-1"
    ></summary>
    <div
      class="book-menu"
      role="menu"
    >
      {#if downloaded || isWebApp}<MenuItemButton
          onclick={(event) => {
            closeMenu(event);
            return onopen(book);
          }}
        >
          {#snippet icon()}
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"
              ><path
                d="M4 5.5h6.5A3.5 3.5 0 0 1 14 9v10.5H7.5A3.5 3.5 0 0 0 4 23zM20 5.5h-2"
              ></path></svg
            >
          {/snippet}
          {BOOK_ACTIONS_CONTRACT.open_label}
        </MenuItemButton>{/if}
      {#if !isWebApp && downloaded}
        <MenuItemButton
          danger
          onclick={(event) => {
            closeMenu(event);
            return onremove(book);
          }}
        >
          {#snippet icon()}
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"
              ><path d="M4 7h16M9 7V4h6v3m-9 0 1 14h10l1-14M10 11v6m4-6v6"></path></svg
            >
          {/snippet}
          {BOOK_ACTIONS_CONTRACT.remove_local_copy_label}
        </MenuItemButton>
      {:else if !isWebApp}
        <MenuItemButton
          disabled={downloading}
          onclick={(event) => {
            closeMenu(event);
            return ondownload(book);
          }}
        >
          {#snippet icon()}
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"><path d="M12 3v12m-5-5 5 5 5-5M5 21h14"></path></svg
            >
          {/snippet}
          {downloading
            ? BOOK_ACTIONS_CONTRACT.download_in_progress_label
            : BOOK_ACTIONS_CONTRACT.download_label}
        </MenuItemButton>
      {/if}
      {#if oncopytofolder}<MenuItemButton
          onclick={(event) => {
            closeMenu(event);
            return oncopytofolder?.(book);
          }}
        >
          {#snippet icon()}
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"
              ><rect
                x="8"
                y="8"
                width="12"
                height="12"
                rx="2"
              ></rect><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"
              ></path></svg
            >
          {/snippet}
          Copy to folder…
        </MenuItemButton>{/if}
      {#if onmovetofolder}<MenuItemButton
          onclick={(event) => {
            closeMenu(event);
            return onmovetofolder?.(book);
          }}
        >
          {#snippet icon()}
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"><path d="M5 12h14m-5-5 5 5-5 5"></path></svg
            >
          {/snippet}
          Move to folder…
        </MenuItemButton>{/if}
      {#if onremovefromfolder}<MenuItemButton
          onclick={(event) => {
            closeMenu(event);
            return onremovefromfolder?.(book);
          }}
        >
          {#snippet icon()}
            <svg
              viewBox="0 0 24 24"
              aria-hidden="true"
              ><circle
                cx="12"
                cy="12"
                r="9"
              ></circle><path d="M8 12h8"></path></svg
            >
          {/snippet}
          Remove from folder
        </MenuItemButton>{/if}
      <MenuItemButton
        danger
        onclick={(event) => {
          closeMenu(event);
          return ontrash(book);
        }}
      >
        {#snippet icon()}
          <svg
            viewBox="0 0 24 24"
            aria-hidden="true"
            ><path d="M4 7h16M9 7V4h6v3m-9 0 1 14h10l1-14M10 11v6m4-6v6"></path></svg
          >
        {/snippet}
        Move to Trash
      </MenuItemButton>
    </div>
  </details>
</div>

<style>
  svg {
    display: block;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: 1.8;
  }
  .book-card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: var(--book-card-cover-content-gap);
    width: 100%;
    min-width: 0;
    color: inherit;
  }
  .book-card.detailed {
    display: grid;
    grid-template-columns: 100px minmax(0, 1fr) auto;
    align-items: start;
    gap: 14px;
    width: 100%;
    height: 178px;
    padding: 9px 14px;
    border-bottom: 1px solid var(--color-border);
  }
  .book-card.detailed:hover {
    background-image: linear-gradient(
      var(--color-hover-overlay),
      var(--color-hover-overlay)
    );
  }
  :global(.cover-button.detailed) {
    width: 100px;
    height: 160px;
  }
  .book-details {
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-width: 0;
    max-width: 640px;
    height: 160px;
    align-content: start;
  }
  .book-meta {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .book-meta strong,
  .book-meta span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .book-meta strong {
    height: 22px;
    font-size: var(--book-card-title-size);
    font-weight: var(--book-card-title-weight);
    line-height: 22px;
    white-space: nowrap;
  }
  .book-meta span {
    height: 20px;
    color: var(--color-subtle-text);
    font-size: var(--book-card-author-size);
    line-height: 20px;
    white-space: nowrap;
  }
  .book-description {
    display: -webkit-box;
    height: 84px;
    -webkit-line-clamp: 4;
    line-clamp: 4;
    -webkit-box-orient: vertical;
    overflow: hidden;
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
    line-height: 1.4;
  }
  .book-cover {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    width: 100%;
    aspect-ratio: 5 / 8;
    overflow: hidden;
    color: var(--color-subtle-text);
  }
  .book-cover.detailed {
    width: 100px;
    height: 160px;
    aspect-ratio: auto;
  }
  .book-cover.missing {
    justify-content: center;
    gap: 5px;
    border: 1px solid var(--color-border);
    background: var(--color-active-highlight);
  }
  .book-cover img {
    display: block;
    width: auto;
    max-width: 100%;
    height: auto;
    max-height: 100%;
    border: 1px solid var(--color-border);
    object-fit: contain;
    box-shadow: 0 4px 12px color-mix(in srgb, var(--color-box-shadow) 40%, transparent);
  }
  .book-cover > small {
    font-size: 0.75rem;
  }
  .card-copy {
    display: grid;
    gap: var(--book-card-text-gap);
    min-width: 0;
  }
  .card-copy strong,
  .card-copy > span {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .card-copy strong {
    display: -webkit-box;
    max-height: var(--book-card-title-block-height);
    font-size: var(--book-card-title-size);
    font-weight: var(--book-card-title-weight);
    line-height: var(--book-card-title-line-height);
    white-space: normal;
    line-clamp: var(--book-card-title-lines);
    -webkit-box-orient: vertical;
    -webkit-line-clamp: var(--book-card-title-lines);
  }
  .card-copy .book-meta span {
    height: var(--book-card-author-block-height);
    color: var(--color-subtle-text);
    font-size: var(--book-card-author-size);
    line-height: var(--book-card-author-line-height);
    white-space: nowrap;
  }
  .opening {
    position: absolute;
    right: 9px;
    bottom: 9px;
    left: 9px;
    padding: 5px;
    border-radius: 0;
    color: var(--color-raised-bg);
    background: color-mix(in srgb, var(--color-ink-text) 78%, transparent);
    text-align: center;
    font-size: 0.75rem;
    font-weight: 600;
  }
  .wide-open {
    display: none;
  }
  .book-actions {
    position: fixed;
    z-index: 40;
    width: 0;
    height: 0;
  }
  .book-actions summary {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    list-style: none;
  }
  .book-actions summary::-webkit-details-marker {
    display: none;
  }
  /* The menu opens on long press, so the finger is still down on an entry:
     without this the WebView starts selecting the entry's label. */
  .book-menu {
    position: absolute;
    top: 0;
    left: 0;
    display: grid;
    width: max-content;
    min-width: 184px;
    padding: 5px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
    box-shadow: 0 13px 32px var(--color-box-shadow);
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
  }
  @media (min-width: 45rem) {
    .book-card {
      width: var(--book-cover-width);
      height: var(--book-card-height);
    }
    .book-card.detailed {
      width: 100%;
      height: 178px;
    }
    :global(.cover-button) {
      width: var(--book-cover-width);
    }
    :global(.cover-button.detailed) {
      width: 100px;
    }
    .book-cover {
      width: var(--book-cover-width);
      height: var(--book-cover-height);
      aspect-ratio: auto;
      border-radius: 0;
    }
    .book-cover.detailed {
      width: 100px;
      height: 160px;
    }
  }
  @media (min-width: 940px) {
    .wide-open {
      display: block;
    }
  }
</style>
