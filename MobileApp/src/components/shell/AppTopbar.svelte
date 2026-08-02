<script lang="ts">
  let {
    libraryName,
    folders,
    showBreadcrumb,
    showLibraryActions,
    addingBook,
    onlibraries,
    onsettings,
    onaddbook,
    onnewfolder,
    onfolder,
  }: {
    libraryName: string;
    folders: { id: string; name: string }[];
    showBreadcrumb: boolean;
    showLibraryActions: boolean;
    addingBook: boolean;
    onlibraries: () => void;
    onsettings: () => void;
    onaddbook: () => void | Promise<void>;
    onnewfolder: () => void;
    onfolder: (level: number) => void | Promise<void>;
  } = $props();
</script>

<header class="topbar">
  <button class="library-button" type="button" aria-haspopup="dialog" onclick={onlibraries}>
    <span>{libraryName}</span>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 9.5 5 5 5-5"></path></svg>
  </button>

  {#if showBreadcrumb}
    <nav class="breadcrumbs" aria-label="Folder breadcrumbs">
      <button type="button" class:current={!folders.length} onclick={() => onfolder(0)}>Library</button>
      {#each folders as folder, index}
        <span aria-hidden="true">/</span>
        <button class:current={index === folders.length - 1} type="button" onclick={() => onfolder(index + 1)}>{folder.name}</button>
      {/each}
    </nav>
  {/if}

  {#if showLibraryActions}
    <div class="library-actions" aria-label="Library actions">
      <button type="button" disabled={addingBook} aria-label={addingBook ? "Adding book" : "Add book"} title={addingBook ? "Adding book…" : "Add book"} onclick={() => void onaddbook()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4.5h10a2 2 0 0 1 2 2V20H7a2 2 0 0 1-2-2zM17 8h2a1 1 0 0 1 1 1v11h-3M11 9v6M8 12h6"></path></svg>
      </button>
      <button type="button" aria-label="New folder" title="New folder" onclick={onnewfolder}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM12 11v7M8.5 14.5h7"></path></svg>
      </button>
    </div>
  {/if}

  <button class="settings-button" type="button" aria-label="Browsing settings" onclick={onsettings}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6l-.05.08h-4l-.05-.08a1.7 1.7 0 0 0-1-.6 1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1l-.08-.05v-4L4 9.9a1.7 1.7 0 0 0 .6-1 1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6l.05-.08h4l.05.08a1.7 1.7 0 0 0 1 .6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.12.4.33.75.6 1l.08.05v4L20 14.1c-.27.25-.48.6-.6.9z"></path></svg>
  </button>
</header>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }
  .topbar { position: sticky; z-index: 20; top: 0; display: flex; flex: none; align-items: center; gap: .75rem; min-height: calc(3.8rem + env(safe-area-inset-top)); padding: calc(.55rem + env(safe-area-inset-top)) var(--page-gutter, 1rem) .55rem; padding-right: max(var(--page-gutter, 1rem), env(safe-area-inset-right)); padding-left: max(var(--page-gutter, 1rem), env(safe-area-inset-left)); border-bottom: 1px solid color-mix(in srgb, var(--color-border-color) 55%, transparent); background: color-mix(in srgb, var(--color-surface-bg) 96%, transparent); backdrop-filter: blur(18px); }
  .library-button { display: flex; flex: none; align-items: center; gap: .35rem; min-width: 0; max-width: min(13rem, 38vw); height: 2.65rem; padding: 0 .65rem; border: 0; border-radius: var(--radius-sm); color: var(--color-ink-text); background: transparent; font-weight: 720; text-align: left; cursor: pointer; }
  .library-button:hover { background: var(--color-hover-bg); }
  .library-button span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .library-button svg { flex: none; width: 1rem; }
  .breadcrumbs { display: flex; flex: 1; align-items: center; justify-content: safe center; gap: .45rem; min-width: 0; min-height: 2.65rem; overflow-x: auto; color: var(--color-disabled-text); scrollbar-width: none; }
  .breadcrumbs::-webkit-scrollbar { display: none; }
  .breadcrumbs button { flex: none; min-height: 2.65rem; padding: .55rem .25rem; border: 0; color: var(--color-subtle-text); background: none; font-size: .86rem; cursor: pointer; }
  .breadcrumbs button.current { color: var(--color-ink-text); font-weight: 750; }
  .library-actions { display: flex; flex: none; align-items: center; gap: .15rem; }
  .library-actions button { display: grid; place-items: center; width: 2.65rem; height: 2.65rem; padding: 0; border: 0; border-radius: var(--radius-sm); color: var(--color-accent); background: transparent; cursor: pointer; }
  .library-actions button:hover { background: var(--color-hover-bg); }
  .library-actions button:disabled { opacity: .5; cursor: default; }
  .library-actions svg { width: 1.2rem; }
  .settings-button { display: grid; flex: none; place-items: center; width: 2.65rem; height: 2.65rem; padding: 0; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; cursor: pointer; }
  .settings-button:hover { color: var(--color-accent); background: var(--color-hover-bg); }
  .settings-button svg { width: 1.2rem; }
  @media (min-width: 45rem) {
    .topbar { min-height: calc(4rem + env(safe-area-inset-top)); }
    .settings-button { display: none; }
  }
</style>
