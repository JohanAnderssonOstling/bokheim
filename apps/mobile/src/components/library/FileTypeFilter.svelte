<script lang="ts">
  import { BROWSING_CONTENT_CONTRACT, type LibraryFileTypeFilter } from "../../lib/types";

  let { value, onchange }: { value: LibraryFileTypeFilter; onchange: (value: LibraryFileTypeFilter) => void | Promise<void> } = $props();
</script>

<div class="file-type-filter" role="group" aria-label="File type">
  {#each BROWSING_CONTENT_CONTRACT.file_type_filters as option}
    <button type="button" class:active={option.value === value} aria-pressed={option.value === value} onclick={() => void onchange(option.value)}>{option.label}</button>
  {/each}
</div>

<style>
  .file-type-filter { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0 18px; }
  button { min-height: 44px; padding: 0 14px; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-subtle-text); background: var(--color-surface); font: 600 max(12px, .75rem) var(--font-interface-family); cursor: pointer; }
  button.active { border-color: var(--color-accent); color: var(--color-ink-text); background: var(--color-selected-surface); }
  button:focus-visible { outline: 2px solid var(--color-accent); outline-offset: 2px; }
</style>
