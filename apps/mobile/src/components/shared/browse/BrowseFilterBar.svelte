<script lang="ts">
  import type { Snippet } from "svelte";
  import type { LibraryFileTypeFilter } from "../../../lib/shared/types";
  import ContextualLibrarySearch from "./ContextualLibrarySearch.svelte";
  import FileTypeFilter from "./FileTypeFilter.svelte";

  let {
    query = $bindable(),
    placeholder,
    fileType,
    onsearch,
    onquerychange,
    onfiletype,
    actions,
  }: {
    query: string;
    placeholder: string;
    fileType: LibraryFileTypeFilter;
    onsearch: () => void;
    onquerychange?: (value: string) => void;
    onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>;
    actions?: Snippet;
  } = $props();
</script>

<div class="browse-filter-bar">
  <div class="search">
    <ContextualLibrarySearch
      bind:value={query}
      {placeholder}
      oninput={(value) => {
        onquerychange?.(value);
        onsearch();
      }}
    />
  </div>
  <div class="controls">
    <FileTypeFilter
      value={fileType}
      onchange={onfiletype}
    />
    {#if actions}
      <div class="actions">{@render actions()}</div>
    {/if}
  </div>
</div>

<style>
  .browse-filter-bar {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: end;
    gap: 10px;
    width: 100%;
    margin: 0 0 var(--content-section-gap, 18px);
  }
  .search {
    width: 100%;
    min-width: 0;
  }
  .actions {
    display: flex;
    gap: 10px;
  }
  .controls {
    display: flex;
    align-items: end;
    gap: 10px;
  }
  @media (max-width: 719px) {
    .controls,
    .actions {
      gap: 0.4rem;
    }
  }
</style>
