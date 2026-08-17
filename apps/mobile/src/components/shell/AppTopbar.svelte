<script lang="ts">
  import { LIBRARY_ACTIONS_CONTRACT } from "../../lib/types";
  import { isWebApp } from "../../lib/platform";
  import FolderBreadcrumbs from "../library/FolderBreadcrumbs.svelte";

  let {
    libraryName,
    pageTitle,
    folders,
    showBreadcrumb,
    showLibraryActions,
    addingBook,
    importingFolder,
    onlibraries,
    onaddbook,
    onnewfolder,
    onimportfolder,
    onfolder,
  }: {
    libraryName: string;
    pageTitle: string;
    folders: { id: string; name: string }[];
    showBreadcrumb: boolean;
    showLibraryActions: boolean;
    addingBook: boolean;
    importingFolder: boolean;
    onlibraries: () => void;
    onaddbook: () => void | Promise<void>;
    onnewfolder: () => void;
    onimportfolder: () => void | Promise<void>;
    onfolder: (level: number) => void | Promise<void>;
  } = $props();

  let actionsOpen = $state(false);
  const nested = $derived(showBreadcrumb && folders.length > 0);
  const mobileTitle = $derived(nested ? folders.at(-1)?.name ?? pageTitle : pageTitle);

  function runAction(action: () => void | Promise<void>) {
    actionsOpen = false;
    void action();
  }

  function dismissActions(event: MouseEvent) {
    if (!(event.target instanceof Element) || !event.target.closest(".mobile-actions")) actionsOpen = false;
  }
</script>

<svelte:window onclick={dismissActions} />

<header class:large-hidden={!showBreadcrumb && !showLibraryActions} class="topbar">
  <div class="mobile-context">
    {#if nested}
      <button class="back-button" type="button" aria-label="Go to parent folder" onclick={() => void onfolder(folders.length - 1)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 18-6-6 6-6"></path></svg>
      </button>
    {:else}
      <button class="library-button" type="button" aria-haspopup="dialog" onclick={onlibraries}>
        <span>{libraryName}</span>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 9.5 5 5 5-5"></path></svg>
      </button>
    {/if}
    <h1>{mobileTitle}</h1>
  </div>

  <div class="desktop-context">
    {#if showBreadcrumb}
      <FolderBreadcrumbs levels={folders} onselect={onfolder} />
    {:else}
      <h1>{pageTitle}</h1>
    {/if}
  </div>

  {#if showLibraryActions}
    <div class="desktop-actions" aria-label="Library actions">
      <button type="button" disabled={addingBook} onclick={() => void onaddbook()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4.5h10a2 2 0 0 1 2 2V20H7a2 2 0 0 1-2-2zM17 8h2a1 1 0 0 1 1 1v11h-3M11 9v6M8 12h6"></path></svg>
        <span>{addingBook ? "Adding…" : LIBRARY_ACTIONS_CONTRACT.add_book_label}</span>
      </button>
      <button type="button" onclick={onnewfolder}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM12 11v7M8.5 14.5h7"></path></svg>
        <span>{LIBRARY_ACTIONS_CONTRACT.new_folder_label}</span>
      </button>
      {#if isWebApp}
        <button type="button" disabled={importingFolder} onclick={() => void onimportfolder()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM12 11v7M8.5 14.5h7M17.5 3v5m-2-2 2 2 2-2"></path></svg>
          <span>{importingFolder ? "Adding…" : "Add folder"}</span>
        </button>
      {/if}
    </div>

    <div class="mobile-actions">
      <button class="add-button" type="button" aria-label="Library actions" aria-haspopup="menu" aria-expanded={actionsOpen} onclick={() => (actionsOpen = !actionsOpen)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14"></path></svg>
      </button>
      {#if actionsOpen}
        <div class="actions-menu" role="menu" aria-label="Library actions">
          <button type="button" role="menuitem" disabled={addingBook} onclick={() => runAction(onaddbook)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4.5h10a2 2 0 0 1 2 2V20H7a2 2 0 0 1-2-2zM17 8h2a1 1 0 0 1 1 1v11h-3M11 9v6M8 12h6"></path></svg>
            <span>{addingBook ? "Adding book…" : LIBRARY_ACTIONS_CONTRACT.add_book_label}</span>
          </button>
          <button type="button" role="menuitem" onclick={() => runAction(onnewfolder)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM12 11v7M8.5 14.5h7"></path></svg>
            <span>{LIBRARY_ACTIONS_CONTRACT.new_folder_label}</span>
          </button>
          {#if isWebApp}
            <button type="button" role="menuitem" disabled={importingFolder} onclick={() => runAction(onimportfolder)}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM17.5 3v6m-3-3 3 3 3-3"></path></svg>
              <span>{importingFolder ? "Adding folder…" : "Add folder from ZIP"}</span>
            </button>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</header>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }
  .topbar { position: sticky; z-index: 20; top: 0; display: flex; flex: none; align-items: center; gap: .55rem; min-height: calc(3.8rem + env(safe-area-inset-top)); padding: calc(.55rem + env(safe-area-inset-top)) max(var(--page-gutter, 1rem), env(safe-area-inset-right)) .55rem max(var(--page-gutter, 1rem), env(safe-area-inset-left)); background: color-mix(in srgb, var(--color-surface-bg) 96%, transparent); backdrop-filter: blur(18px); }
  .mobile-context { display: flex; flex: 1; align-items: center; min-width: 0; gap: .25rem; }
  .desktop-context, .desktop-actions { display: none; }
  .library-button, .back-button, .add-button { display: flex; flex: none; align-items: center; justify-content: center; height: 2.65rem; padding: 0; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; cursor: pointer; }
  .library-button { max-width: min(10rem, 43vw); justify-content: start; gap: .3rem; padding: 0 .35rem; font-weight: 720; }
  .library-button span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .library-button svg { flex: none; width: .95rem; }
  .back-button { width: 2.65rem; color: var(--color-accent); }
  .back-button svg, .add-button svg { width: 1.35rem; }
  h1 { flex: 1; min-width: 0; margin: 0; overflow: hidden; color: var(--color-ink-text); font: 600 1.05rem var(--font-interface-family); text-overflow: ellipsis; white-space: nowrap; }
  .mobile-actions { position: relative; flex: none; }
  .add-button { width: 2.65rem; color: var(--color-accent); background: var(--color-active-highlight); }
  .library-button:hover, .back-button:hover, .add-button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .actions-menu { position: absolute; z-index: 2; top: calc(100% + .45rem); right: 0; display: grid; width: min(17rem, calc(100vw - 2rem)); padding: .4rem; border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); box-shadow: 0 .8rem 2rem color-mix(in srgb, var(--color-box-shadow) 35%, transparent); }
  .actions-menu button { display: grid; grid-template-columns: 1.5rem minmax(0, 1fr); align-items: center; gap: .7rem; min-height: 3rem; padding: .5rem .7rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; text-align: left; cursor: pointer; }
  .actions-menu button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .actions-menu button:disabled { opacity: .5; cursor: default; }
  .actions-menu svg { width: 1.25rem; color: var(--color-accent); }

  @media (min-width: 45rem) {
    .topbar { min-height: calc(var(--layout-topbar-height) + env(safe-area-inset-top)); gap: .75rem; }
    .topbar.large-hidden { display: none; }
    .mobile-context, .mobile-actions { display: none; }
    .desktop-context { display: flex; flex: 1; min-width: 0; }
    .desktop-context h1 { font-size: 1.25rem; font-weight: 500; }
    .desktop-actions { display: flex; flex: none; align-items: center; gap: var(--layout-topbar-action-gap); }
    .desktop-actions button { display: inline-flex; align-items: center; justify-content: center; gap: .4rem; height: var(--layout-topbar-action-height); padding: 0 var(--layout-topbar-action-horizontal-padding); border: 1px solid var(--color-border); border-radius: 0; color: var(--color-accent); background: var(--color-raised-bg); font-size: .8125rem; font-weight: var(--topbar-action-label-weight); cursor: pointer; }
    .desktop-actions button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
    .desktop-actions button:disabled { opacity: .5; cursor: default; }
    .desktop-actions svg { width: 1.2rem; }
  }
</style>
