<script lang="ts">
  import { BROWSING_CONTENT_CONTRACT, type LibraryFileTypeFilter } from "../../lib/types";
  import { managedMenu } from "../../lib/keyboard-navigation";

  let { value, onchange }: { value: LibraryFileTypeFilter; onchange: (value: LibraryFileTypeFilter) => void | Promise<void> } = $props();
  let menu = $state<HTMLDetailsElement>();
  const selectedLabel = $derived(BROWSING_CONTENT_CONTRACT.file_type_filters.find((option) => option.value === value)?.label ?? "All");

  function select(next: LibraryFileTypeFilter) {
    if (menu) {
      menu.open = false;
      menu.querySelector<HTMLElement>("summary")?.focus();
    }
    void onchange(next);
  }
</script>

<div class="file-type-filter">
  <details bind:this={menu} use:managedMenu>
    <summary aria-label={`File format: ${selectedLabel}`}>
      <span>{selectedLabel}</span>
      <span class="caret" aria-hidden="true">⌄</span>
    </summary>
    <div class="format-menu" role="menu" aria-label="File format">
      {#each BROWSING_CONTENT_CONTRACT.file_type_filters as option}
        <button type="button" role="menuitemradio" aria-checked={option.value === value} onclick={() => select(option.value)}>
          <span class="check" aria-hidden="true">✓</span>
          <span>{option.label}</span>
        </button>
      {/each}
    </div>
  </details>
</div>

<style>
  .file-type-filter { width: auto; min-width: 0; }
  details { position: relative; width: auto; }
  summary { box-sizing: border-box; display: flex; align-items: center; justify-content: space-between; gap: .75rem; width: auto; min-width: 4rem; min-height: var(--layout-topbar-action-height); padding: 0 .75rem; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-ink-text); background: transparent; font: 700 max(12px, .75rem) var(--font-interface-family); cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
  details[open] summary { border-color: var(--color-accent); }
  .caret { color: var(--color-subtle-text); font-size: 1rem; transition: transform 120ms ease; }
  details[open] .caret { transform: rotate(180deg); }
  .format-menu { position: absolute; z-index: 20; top: calc(100% + 4px); right: 0; display: grid; width: max-content; min-width: 11rem; padding: 4px; border: 1px solid var(--color-border); background: var(--color-raised-bg); box-shadow: 0 .7rem 1.8rem var(--color-box-shadow); }
  button { display: grid; grid-template-columns: 1.25rem minmax(0, 1fr); align-items: center; min-height: 42px; padding: .5rem .65rem; border: 0; border-radius: 0; color: var(--color-ink-text); background: transparent; text-align: left; font: 650 max(12px, .74rem) var(--font-interface-family); cursor: pointer; }
  button:hover, button:focus-visible { background: var(--color-selected-surface); outline: 0; }
  button[aria-checked="true"] { color: var(--color-accent); font-weight: 750; }
  .check { visibility: hidden; font-size: .9rem; }
  button[aria-checked="true"] .check { visibility: visible; }

  @media (max-width: 719px) {
    .file-type-filter { width: 8.5rem; }
    details, summary { width: 100%; }
    summary { min-height: 48px; }
    .format-menu { right: auto; left: 0; min-width: 100%; }
    button { min-height: 48px; }
  }
</style>
