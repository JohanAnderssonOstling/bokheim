<script lang="ts">
  import { ACCOUNT_CONTRACT, LIBRARY_ACTIONS_CONTRACT, SETTINGS_CONTRACT } from "../../lib/types";
  import FolderBreadcrumbs from "../library/FolderBreadcrumbs.svelte";
  let {
    libraryName,
    pageTitle,
    folders,
    showBreadcrumb,
    showLibraryActions,
    addingBook,
    accountSignedIn,
    onlibraries,
    onaccount,
    onsettings,
    onaddbook,
    onnewfolder,
    onfolder,
  }: {
    libraryName: string;
    pageTitle: string;
    folders: { id: string; name: string }[];
    showBreadcrumb: boolean;
    showLibraryActions: boolean;
    addingBook: boolean;
    accountSignedIn: boolean;
    onlibraries: () => void;
    onaccount: () => void;
    onsettings: () => void;
    onaddbook: () => void | Promise<void>;
    onnewfolder: () => void;
    onfolder: (level: number) => void | Promise<void>;
  } = $props();
</script>

<header class:large-hidden={!showBreadcrumb && !showLibraryActions} class="topbar">
  <button class="library-button" type="button" aria-haspopup="dialog" onclick={onlibraries}>
    <span>{libraryName}</span>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 9.5 5 5 5-5"></path></svg>
  </button>

  {#if showBreadcrumb}
    <FolderBreadcrumbs levels={folders} onselect={onfolder} />
  {:else}
    <h1>{pageTitle}</h1>
  {/if}

  {#if showLibraryActions}
    <div class="library-actions" aria-label="Library actions">
      <button type="button" disabled={addingBook} aria-label={addingBook ? "Adding book" : LIBRARY_ACTIONS_CONTRACT.add_book_label} title={addingBook ? "Adding book…" : LIBRARY_ACTIONS_CONTRACT.add_book_label} onclick={() => void onaddbook()}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 4.5h10a2 2 0 0 1 2 2V20H7a2 2 0 0 1-2-2zM17 8h2a1 1 0 0 1 1 1v11h-3M11 9v6M8 12h6"></path></svg>
        <span>{addingBook ? "Adding…" : LIBRARY_ACTIONS_CONTRACT.add_book_label}</span>
      </button>
      <button type="button" aria-label={LIBRARY_ACTIONS_CONTRACT.new_folder_label} title={LIBRARY_ACTIONS_CONTRACT.new_folder_label} onclick={onnewfolder}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2zM12 11v7M8.5 14.5h7"></path></svg>
        <span>{LIBRARY_ACTIONS_CONTRACT.new_folder_label}</span>
      </button>
    </div>
  {/if}

  <button class="account-button" type="button" aria-label={accountSignedIn ? ACCOUNT_CONTRACT.navigation_label : "Sign in"} onclick={onaccount}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 12.5a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM4.5 21a7.5 7.5 0 0 1 15 0"></path></svg>
  </button>
  <button class="settings-button" type="button" aria-label={SETTINGS_CONTRACT.navigation_label} onclick={onsettings}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6l-.05.08h-4l-.05-.08a1.7 1.7 0 0 0-1-.6 1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1l-.08-.05v-4L4 9.9a1.7 1.7 0 0 0 .6-1 1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6l.05-.08h4l.05.08a1.7 1.7 0 0 0 1 .6 1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.12.4.33.75.6 1l.08.05v4L20 14.1c-.27.25-.48.6-.6.9z"></path></svg>
  </button>
</header>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  svg { display: block; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.8; }
  .topbar { position: sticky; z-index: 20; top: 0; display: flex; flex: none; align-items: center; gap: .75rem; min-height: calc(3.8rem + env(safe-area-inset-top)); padding: calc(.55rem + env(safe-area-inset-top)) var(--page-gutter, 1rem) .55rem; padding-right: max(var(--page-gutter, 1rem), env(safe-area-inset-right)); padding-left: max(var(--page-gutter, 1rem), env(safe-area-inset-left)); border-bottom: 1px solid color-mix(in srgb, var(--color-border) 55%, transparent); background: color-mix(in srgb, var(--color-surface-bg) 96%, transparent); backdrop-filter: blur(18px); }
  .library-button { display: flex; flex: none; align-items: center; gap: .35rem; min-width: 0; max-width: min(13rem, 38vw); height: 2.65rem; padding: 0 .65rem; border: 0; border-radius: var(--radius-sm); color: var(--color-ink-text); background: transparent; font-weight: 720; text-align: left; cursor: pointer; }
  .library-button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .library-button span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .library-button svg { flex: none; width: 1rem; }
  h1 { flex: 1; min-width: 0; margin: 0; overflow: hidden; color: var(--color-ink-text); font: 500 1.25rem var(--font-interface-family); text-overflow: ellipsis; white-space: nowrap; }
  .library-actions { display: flex; flex: none; align-items: center; gap: var(--layout-topbar-action-gap); }
  .library-actions button { display: inline-flex; align-items: center; justify-content: center; gap: .4rem; width: auto; height: var(--layout-topbar-action-height); padding: 0 var(--layout-topbar-action-horizontal-padding); border: 1px solid var(--color-border); border-radius: var(--radius-sm); color: var(--color-accent); background: var(--color-raised-bg); font-size: .8125rem; font-weight: 750; cursor: pointer; }
  .library-actions button:hover { background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .library-actions button:disabled { opacity: .5; cursor: default; }
  .library-actions svg { width: 1.2rem; }
  .library-actions button span { display: none; }
  .account-button, .settings-button { display: grid; flex: none; place-items: center; width: 2.65rem; height: 2.65rem; padding: 0; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; cursor: pointer; }
  .account-button:hover, .settings-button:hover { color: var(--color-accent); background-image: linear-gradient(var(--color-hover-overlay), var(--color-hover-overlay)); }
  .account-button svg, .settings-button svg { width: 1.2rem; }
  @media (min-width: 45rem) {
    .topbar { min-height: calc(var(--layout-topbar-height) + env(safe-area-inset-top)); }
    .topbar.large-hidden { display: none; }
    .library-button, .account-button, .settings-button { display: none; }
    .library-actions button span { display: inline; }
  }
</style>
