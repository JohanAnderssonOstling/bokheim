<script lang="ts">
  import { tick } from "svelte";

  import type { FolderDownloadStatus, LibraryDirectory } from "../../lib/types";
  import { isWebApp } from "../../lib/platform";
  import DownloadIndicator from "./DownloadIndicator.svelte";
  import FolderNavigationCard from "./FolderNavigationCard.svelte";
  import { managedMenu } from "../../lib/keyboard-navigation";

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
    if (Math.hypot(event.clientX - longPressOrigin.x, event.clientY - longPressOrigin.y) > 8) cancelLongPress();
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
    if (!isWebApp && total > 0 && !complete) void ondownload(folder);
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
    reserveEnd={!isWebApp && total > 0}
    flat
    onclick={openFolder}
    onpointerdown={startLongPress}
    onpointerup={cancelLongPress}
    onpointercancel={cancelLongPress}
    onpointermove={trackLongPress}
    oncontextmenu={openContextMenu}
    onkeydown={handleKeydown}
  />
  {#if !isWebApp && total > 0}
    <div class="download-action">
      <DownloadIndicator
        {progress}
        {active}
        {complete}
        label={complete ? `${folder.name} downloaded` : `Download ${folder.name}, ${downloaded} of ${total} books downloaded`}
      />
    </div>
  {/if}
  <details class="folder-actions" style={`left: ${menuPosition.x}px; top: ${menuPosition.y}px;`} bind:this={actions} use:managedMenu>
    <summary aria-label={`Folder actions for ${folder.name}`} tabindex="-1"></summary>
    <div class="folder-menu" role="menu">
      <button type="button" role="menuitem" onclick={(event) => { closeMenu(event); return onrename(folder); }}>Rename</button>
      <button type="button" role="menuitem" onclick={(event) => { closeMenu(event); return onmove(folder); }}>Move to folder…</button>
      <button class="danger" type="button" role="menuitem" onclick={(event) => { closeMenu(event); return ontrash(folder); }}>Move to Trash</button>
    </div>
  </details>
</div>

<style>
  .folder-card { position: relative; width: auto; min-width: 0; }
  .download-action { position: absolute; top: 50%; right: .55rem; display: grid; width: 2.75rem; height: 2.75rem; place-items: center; transform: translateY(-50%); pointer-events: none; }
  .folder-actions { position: fixed; z-index: 40; width: 0; height: 0; }
  .folder-actions summary { position: absolute; width: 1px; height: 1px; padding: 0; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; list-style: none; }
  .folder-actions summary::-webkit-details-marker { display: none; }
  .folder-menu { position: absolute; top: 0; left: 0; display: grid; width: max-content; min-width: 11rem; padding: .35rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); box-shadow: 0 .8rem 2rem var(--color-box-shadow); }
  .folder-menu button { min-height: 2.5rem; padding: .55rem .7rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; text-align: left; font-size: .76rem; font-weight: 650; cursor: pointer; }
  .folder-menu button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .folder-menu button.danger { color: var(--color-danger-text); }
</style>
