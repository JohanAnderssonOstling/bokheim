<script lang="ts">
  import { tick } from "svelte";

  import type {
    FolderDownloadStatus,
    LibraryDirectory,
  } from "../../../../../lib/shared/types";
  import { isWebApp } from "../../../../../lib/app/platform";
  import DownloadIndicator from "../../../../../components/shared/books/DownloadIndicator.svelte";
  import MenuItemButton from "../../../../../components/shared/buttons/MenuItemButton.svelte";
  import FolderNavigationCard from "../../../../../components/shared/browse/FolderNavigationCard.svelte";
  import { managedMenu } from "../../../../../lib/app/keyboard-navigation";

  let {
    folder,
    status,
    active = false,
    onopen,
    ondownload,
    ontrash,
    onrename,
    onmove,
  }: {
    folder: LibraryDirectory;
    status?: FolderDownloadStatus;
    active?: boolean;
    onopen: (folder: LibraryDirectory) => void | Promise<void>;
    ondownload: (folder: LibraryDirectory) => void | Promise<void>;
    ontrash: (folder: LibraryDirectory) => void | Promise<void>;
    onrename: (folder: LibraryDirectory) => void | Promise<void>;
    onmove: (folder: LibraryDirectory) => void | Promise<void>;
  } = $props();

  const total = $derived(status?.total_books ?? folder.book_count);
  const downloaded = $derived(status?.downloaded_books ?? 0);
  const progress = $derived(total ? downloaded / total : 0);
  const complete = $derived(total > 0 && downloaded >= total);
  // A fully downloaded folder carries no download affordance at all; a partly
  // downloaded one still offers the rest.
  const showDownloadAction = $derived(!isWebApp && total > 0 && (active || !complete));
  let actions = $state<HTMLDetailsElement>();
  let menuPosition = $state({ x: 0, y: 0 });
  let longPressTimer: ReturnType<typeof setTimeout> | undefined;
  let longPressOrigin: { x: number; y: number } | undefined;
  let suppressOpenUntil = 0;

  async function showActions(x: number, y: number) {
    suppressOpenUntil = Date.now() + 800;
    if (!actions) return;
    menuPosition = { x, y };
    actions.open = true;
    await tick();
    const menu = actions.querySelector<HTMLElement>(".folder-menu");
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

  function openFolder(event: MouseEvent) {
    if (Date.now() < suppressOpenUntil) {
      suppressOpenUntil = 0;
      event.preventDefault();
      return;
    }
    return onopen(folder);
  }

  function closeMenu(event: MouseEvent) {
    (event.currentTarget as HTMLElement).closest("details")?.removeAttribute("open");
  }
</script>

<div class="folder-card">
  <FolderNavigationCard
    name={folder.name}
    count={folder.book_count}
    reserveEnd={showDownloadAction}
    flat
    onclick={openFolder}
    onpointerdown={startLongPress}
    onpointerup={cancelLongPress}
    onpointercancel={cancelLongPress}
    onpointermove={trackLongPress}
    oncontextmenu={openContextMenu}
    onkeydown={handleKeydown}
  />
  {#if showDownloadAction}
    <div class="download-action">
      <DownloadIndicator
        {progress}
        {active}
        label={active
          ? `Downloading ${folder.name}, ${downloaded} of ${total} books downloaded`
          : `Download ${folder.name}`}
      />
    </div>
  {/if}
  <details
    class="folder-actions"
    style={`left: ${menuPosition.x}px; top: ${menuPosition.y}px;`}
    bind:this={actions}
    use:managedMenu
  >
    <summary
      aria-label={`Folder actions for ${folder.name}`}
      tabindex="-1"
    ></summary>
    <div
      class="folder-menu"
      role="menu"
    >
      {#if !isWebApp && total > 0 && !complete}
        <MenuItemButton
          disabled={active}
          onclick={(event) => {
            closeMenu(event);
            return ondownload(folder);
          }}>{active ? "Downloading…" : "Download"}</MenuItemButton
        >
      {/if}
      <MenuItemButton
        onclick={(event) => {
          closeMenu(event);
          return onrename(folder);
        }}>Rename</MenuItemButton
      >
      <MenuItemButton
        onclick={(event) => {
          closeMenu(event);
          return onmove(folder);
        }}>Move to folder…</MenuItemButton
      >
      <MenuItemButton
        danger
        onclick={(event) => {
          closeMenu(event);
          return ontrash(folder);
        }}>Move to Trash</MenuItemButton
      >
    </div>
  </details>
</div>

<style>
  .folder-card {
    position: relative;
    width: auto;
    min-width: 0;
  }
  .download-action {
    position: absolute;
    top: 50%;
    right: 0.55rem;
    display: grid;
    width: 2.75rem;
    height: 2.75rem;
    place-items: center;
    transform: translateY(-50%);
    pointer-events: none;
  }
  .folder-actions {
    position: fixed;
    z-index: 40;
    width: 0;
    height: 0;
  }
  .folder-actions summary {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    list-style: none;
  }
  .folder-actions summary::-webkit-details-marker {
    display: none;
  }
  /* Opened by long press, so the finger rests on an entry: keep the WebView
     from selecting the entry's label. */
  .folder-menu {
    position: absolute;
    top: 0;
    left: 0;
    display: grid;
    width: max-content;
    min-width: 11rem;
    padding: 0.35rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
    box-shadow: 0 0.8rem 2rem var(--color-box-shadow);
    user-select: none;
    -webkit-user-select: none;
    -webkit-touch-callout: none;
  }
</style>
