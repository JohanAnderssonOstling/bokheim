<script lang="ts">
  import type { LibraryFileTypeFilter } from "../../lib/types";
  import ContextualLibrarySearch from "./ContextualLibrarySearch.svelte";
  import FileTypeFilter from "./FileTypeFilter.svelte";

  let { query = $bindable(), placeholder, fileType, onsearch, onfiletype }: {
    query: string;
    placeholder: string;
    fileType: LibraryFileTypeFilter;
    onsearch: () => void;
    onfiletype: (value: LibraryFileTypeFilter) => void | Promise<void>;
  } = $props();
</script>

<div class="browse-filter-bar">
  <div class="search"><ContextualLibrarySearch bind:value={query} {placeholder} oninput={onsearch} /></div>
  <FileTypeFilter value={fileType} onchange={onfiletype} />
</div>

<style>
  .browse-filter-bar { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: end; gap: 10px; width: 100%; margin: 0 0 18px; }
  .search { width: 100%; max-width: 30rem; min-width: 0; }
</style>
