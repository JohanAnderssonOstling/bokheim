<script lang="ts">
  import SquareButton from "../shared/buttons/SquareButton.svelte";
  import MenuItemButton from "../shared/buttons/MenuItemButton.svelte";
  import { Plus, Trash2, X } from "@lucide/svelte";
  import type { LibraryEntry } from "../../lib/shared/types";
  import { arrowKeyNavigation, modalFocus } from "../../lib/app/keyboard-navigation";
  import { isWebApp } from "../../lib/app/platform";

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

  const sorter = new Intl.Collator(undefined, {
    sensitivity: "base",
    numeric: true,
    ignorePunctuation: true,
  });
  const sortedLibraries = $derived(
    [...libraries].sort((left, right) =>
      sorter.compare(left.library_name, right.library_name),
    ),
  );
  let addMenuOpen = $state(false);
</script>

<button
  class="scrim"
  type="button"
  aria-label="Close library switcher"
  onclick={onclose}
></button>
<div
  class="sheet"
  role="dialog"
  aria-modal="true"
  aria-labelledby="switcher-title"
  use:modalFocus={{
    onClose: onclose,
    initialFocus: ".library-select.active, .library-select",
  }}
>
  <header>
    <h2 id="switcher-title">Libraries</h2>
    <div class="header-actions">
      <div class="add-action">
        <SquareButton
          label="Add library"
          ariaExpanded={addMenuOpen}
          ariaHasPopup="menu"
          onclick={() => (addMenuOpen = !addMenuOpen)}
          ><Plus
            size={20}
            strokeWidth={1.8}
          /></SquareButton
        >
        {#if addMenuOpen}
          <div
            class="add-menu"
            role="menu"
          >
            <MenuItemButton
              onclick={() => {
                addMenuOpen = false;
                oncreate();
              }}>Create new</MenuItemButton
            >
            <MenuItemButton
              disabled={importingArchive}
              onclick={() => {
                addMenuOpen = false;
                void (isWebApp ? onimportarchive() : onopen());
              }}>{importingArchive ? "Opening…" : "Open library"}</MenuItemButton
            >
          </div>
        {/if}
      </div>
      <SquareButton
        label="Close"
        onclick={onclose}
        ><X
          size={20}
          strokeWidth={1.8}
        /></SquareButton
      >
    </div>
  </header>
  <div
    class="list"
    use:arrowKeyNavigation={{ orientation: "vertical", selector: ".library-select" }}
  >
    {#each sortedLibraries as library}
      <div class="library-row">
        <button
          class:active={library.library_id === activeLibraryId}
          class="library-select"
          type="button"
          aria-current={library.library_id === activeLibraryId ? "true" : undefined}
          onclick={() => onselect(library.library_id)}
        >
          <span>{library.library_name}</span>
        </button>
        <button
          class="library-remove"
          aria-label={`Delete ${library.library_name}`}
          type="button"
          onclick={() => void onremove(library.library_id)}
        >
          <Trash2
            size={16}
            strokeWidth={1.8}
          />
        </button>
      </div>
    {/each}
  </div>
</div>

<style>
  button {
    font: inherit;
    -webkit-tap-highlight-color: transparent;
  }
  .scrim {
    position: fixed;
    z-index: 90;
    inset: 0;
    width: 100%;
    height: 100%;
    border: 0;
    background: var(--color-overlay-scrim);
  }
  .sheet {
    position: fixed;
    z-index: 91;
    inset: 50% auto auto 50%;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    gap: 0.25rem;
    width: min(20.5rem, calc(100vw - 2rem));
    max-height: min(78dvh, 36rem);
    overflow: hidden;
    padding: 0.75rem;
    transform: translate(-50%, -50%);
    border: 1px solid var(--color-border);
    border-radius: 0.375rem;
    background: var(--color-raised-bg);
    box-shadow: 0 1.2rem 3rem color-mix(in srgb, var(--color-box-shadow) 55%, transparent);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    height: 2.5rem;
    gap: 0.6rem;
    padding: 0 0 0.25rem 0.5rem;
  }
  h2 {
    margin: 0;
    color: var(--color-ink-text);
    font: 700 1rem var(--font-interface-family);
  }
  .header-actions {
    display: flex;
    flex: none;
    align-items: center;
    gap: 0.45rem;
  }
  .add-action {
    position: relative;
  }
  .list {
    display: grid;
    min-height: 0;
    gap: 0.25rem;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .library-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 2.5rem;
    gap: 0.25rem;
  }
  .library-select {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    width: 100%;
    min-width: 0;
    height: 2.5rem;
    padding: 0 0.5rem;
    overflow: hidden;
    border: 1px solid transparent;
    border-radius: 0.125rem;
    color: var(--color-ink-text);
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .library-select span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .library-select.active {
    color: var(--color-accent);
    background: var(--color-active-highlight);
    font-weight: 750;
  }
  .library-remove {
    display: grid;
    place-items: center;
    width: 2.5rem;
    height: 2.5rem;
    padding: 0;
    border: 1px solid var(--color-border);
    border-radius: 0.125rem;
    color: var(--color-subtle-text);
    background: var(--color-raised-bg);
    cursor: pointer;
  }
  .add-menu {
    position: absolute;
    z-index: 2;
    top: calc(100% + 0.35rem);
    right: 0;
    display: grid;
    width: 9rem;
    padding: 0.25rem;
    border: 1px solid var(--color-border);
    border-radius: 0.25rem;
    background: var(--color-raised-bg);
    box-shadow: 0 0.8rem 2rem color-mix(in srgb, var(--color-box-shadow) 35%, transparent);
  }
  @media (min-width: 45rem) {
    .sheet {
      top: 50%;
      width: min(20.5rem, calc(100vw - 2rem));
    }
  }
</style>
