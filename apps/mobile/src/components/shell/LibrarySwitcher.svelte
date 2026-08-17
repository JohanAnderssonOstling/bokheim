<script lang="ts">
  import type { LibraryEntry } from "../../lib/types";
  import { arrowKeyNavigation, modalFocus } from "../../lib/keyboard-navigation";
  import { isWebApp } from "../../lib/platform";

  let {
    libraries,
    activeLibraryId,
    onclose,
    onselect,
    oncreate,
    onopen,
    onimportarchive,
    importingArchive,
    onremove,
  }: {
    libraries: LibraryEntry[];
    activeLibraryId: string;
    onclose: () => void;
    onselect: (libraryId: string) => void | Promise<void>;
    oncreate: () => void;
    onopen: () => void | Promise<void>;
    onimportarchive: () => void | Promise<void>;
    importingArchive: boolean;
    onremove: (libraryId: string) => void | Promise<void>;
  } = $props();
</script>

<button class="scrim" type="button" aria-label="Close library switcher" onclick={onclose}></button>
<div class="sheet" role="dialog" aria-modal="true" aria-labelledby="switcher-title" use:modalFocus={{ onClose: onclose, initialFocus: ".library-select.active, .library-select" }}>
  <header>
    <h2 id="switcher-title">Switch library</h2>
    <button type="button" aria-label="Close" onclick={onclose}>×</button>
  </header>
  <div class="list" use:arrowKeyNavigation={{ orientation: "vertical", selector: ".library-select" }}>
    {#each libraries as library}
      <button
        class:active={library.library_id === activeLibraryId}
        class="library-select"
        type="button"
        aria-current={library.library_id === activeLibraryId ? "true" : undefined}
        onclick={() => onselect(library.library_id)}
      >
        <span>{library.library_name}</span>
        {#if library.library_id === activeLibraryId}<span aria-hidden="true">✓</span>{/if}
      </button>
    {/each}
  </div>
  <div class="library-actions">
    <p>Library actions</p>
    <button type="button" onclick={oncreate}>Create library</button>
    {#if isWebApp}<button type="button" disabled={importingArchive} onclick={() => void onimportarchive()}>{importingArchive ? "Opening library…" : "Open library"}</button>{/if}
    {#if !isWebApp}<button type="button" onclick={() => void onopen()}>Open library</button>{/if}
    {#if activeLibraryId}
      <button class="danger" type="button" onclick={() => void onremove(activeLibraryId)}>Delete selected library</button>
    {/if}
  </div>
</div>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  .scrim { position: fixed; z-index: 90; inset: 0; width: 100%; height: 100%; border: 0; background: var(--color-overlay-scrim); }
  .sheet { position: fixed; z-index: 91; right: 0; bottom: 0; left: 0; max-height: min(78dvh, 38rem); padding: 1rem max(1rem, env(safe-area-inset-right)) calc(1rem + env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left)); border: 1px solid var(--color-border); border-radius: 0; background: var(--color-raised-bg); box-shadow: 0 -1rem 3rem color-mix(in srgb, var(--color-box-shadow) 55%, transparent); }
  header { display: flex; align-items: center; justify-content: space-between; min-height: 2.6rem; margin-bottom: .5rem; }
  h2 { margin: 0; font: 500 1.3rem var(--font-interface-family); }
  header button { width: 2.5rem; height: 2.5rem; border: 0; border-radius: 0; color: var(--color-subtle-text); background: transparent; font-size: 1.5rem; }
  .list { display: grid; max-height: 48dvh; overflow-y: auto; }
  .library-select { display: flex; align-items: center; justify-content: space-between; gap: 1rem; min-width: 0; min-height: 3.25rem; padding: .55rem .7rem; overflow: hidden; border: 0; border-bottom: 1px solid var(--color-border); color: var(--color-ink-text); background: transparent; text-align: left; cursor: pointer; }
  .library-select span:first-child { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .library-select.active { color: var(--color-accent); background: var(--color-active-highlight); font-weight: 750; }
  .library-actions { display: grid; grid-template-columns: 1fr 1fr; gap: .55rem; margin-top: .8rem; padding-top: .8rem; border-top: 1px solid var(--color-border); }
  .library-actions p { grid-column: 1 / -1; margin: 0; color: var(--color-subtle-text); font-size: .68rem; font-weight: 800; letter-spacing: .04em; text-transform: uppercase; }
  .library-actions button { min-height: 2.8rem; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-accent); background: transparent; font-size: .78rem; font-weight: 750; cursor: pointer; }
  .library-actions button:disabled { opacity: .55; cursor: default; }
  .library-actions button.danger { grid-column: 1 / -1; color: var(--color-danger-text); }
  @media (min-width: 45rem) { .sheet { top: 50%; right: auto; bottom: auto; left: 50%; width: min(30rem, calc(100vw - 2rem)); padding: 1.1rem; transform: translate(-50%, -50%); } }
</style>
