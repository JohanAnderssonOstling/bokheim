<script lang="ts">
  import type { LibraryEntry } from "../../lib/types";

  let {
    libraries,
    activeLibraryId,
    onclose,
    onselect,
    onopenfile,
  }: {
    libraries: LibraryEntry[];
    activeLibraryId: string;
    onclose: () => void;
    onselect: (libraryId: string) => void | Promise<void>;
    onopenfile: () => void | Promise<void>;
  } = $props();
</script>

<button class="scrim" type="button" aria-label="Close library switcher" onclick={onclose}></button>
<div class="sheet" role="dialog" aria-modal="true" aria-labelledby="switcher-title">
  <header>
    <h2 id="switcher-title">Switch library</h2>
    <button type="button" aria-label="Close" onclick={onclose}>×</button>
  </header>
  <div class="list">
    {#each libraries as library}
      <button class:active={library.library_id === activeLibraryId} type="button" onclick={() => onselect(library.library_id)}>
        <span>{library.library_name}</span>
        {#if library.library_id === activeLibraryId}<span aria-hidden="true">✓</span>{/if}
      </button>
    {/each}
  </div>
  <button class="open-file" type="button" onclick={() => void onopenfile()}>
    Open EPUB
  </button>
</div>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  .scrim { position: fixed; z-index: 90; inset: 0; width: 100%; height: 100%; border: 0; background: var(--color-overlay-scrim); }
  .sheet { position: fixed; z-index: 91; right: 0; bottom: 0; left: 0; max-height: min(78dvh, 38rem); padding: 1rem max(1rem, env(safe-area-inset-right)) calc(1rem + env(safe-area-inset-bottom)) max(1rem, env(safe-area-inset-left)); border: 1px solid var(--color-border-color); border-radius: var(--radius-lg) var(--radius-lg) 0 0; background: var(--color-white-surface); box-shadow: 0 -1rem 3rem color-mix(in srgb, var(--color-box-shadow) 55%, transparent); }
  header { display: flex; align-items: center; justify-content: space-between; min-height: 2.6rem; margin-bottom: .5rem; }
  h2 { margin: 0; font: 500 1.3rem var(--font-heading-family); }
  header button { width: 2.5rem; height: 2.5rem; border: 0; border-radius: var(--radius-sm); color: var(--color-subtle-text); background: transparent; font-size: 1.5rem; }
  .list { display: grid; max-height: 48dvh; overflow-y: auto; }
  .list button { display: flex; align-items: center; justify-content: space-between; gap: 1rem; min-height: 3rem; padding: .55rem .7rem; border: 0; border-bottom: 1px solid var(--color-border-color); color: var(--color-ink-text); background: transparent; text-align: left; }
  .list button.active { color: var(--color-accent); background: var(--color-active-highlight); font-weight: 750; }
  .open-file { display: grid; place-items: center; width: 100%; min-height: 2.8rem; margin-top: .8rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: var(--color-accent); background: transparent; font-size: .78rem; font-weight: 750; cursor: pointer; }
  @media (min-width: 45rem) { .sheet { top: 50%; right: auto; bottom: auto; left: 50%; width: min(30rem, calc(100vw - 2rem)); padding: 1.1rem; border-radius: .75rem; transform: translate(-50%, -50%); } }
</style>
