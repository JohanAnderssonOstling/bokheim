<script lang="ts">
  import SquareButton from "../../../../../components/shared/buttons/SquareButton.svelte";
  import SearchField from "../../../../../components/shared/SearchField.svelte";
  import type { SearchResult } from "../pdf-reader-types";

  let {
    searchOpen,
    searchQuery,
    searchResults,
    selectedSearchResult,
    searching,
    onQueryInput,
    onSubmitSearch,
    onPrevious,
    onNext,
    onClose,
  }: {
    searchOpen: boolean;
    searchQuery: string;
    searchResults: SearchResult[];
    selectedSearchResult: number;
    searching: boolean;
    onQueryInput: (query: string) => void;
    onSubmitSearch: () => void;
    onPrevious: () => void;
    onNext: () => void;
    onClose: () => void;
  } = $props();
</script>

{#if searchOpen}
  <section
    class="pdf-search"
    aria-label="Search PDF"
  >
    <SearchField
      class="pdf-search-form"
      showIcon={false}
      value={searchQuery}
      placeholder="Search this PDF"
      ariaLabel="Search PDF text"
      oninput={onQueryInput}
      onsubmit={onSubmitSearch}
    >
      <output
        >{searchResults.length
          ? `${selectedSearchResult + 1}/${searchResults.length}`
          : "0/0"}</output
      >
      <SquareButton
        label="Previous search result"
        disabled={!searchResults.length}
        onclick={onPrevious}>‹</SquareButton
      >
      <SquareButton
        label="Next search result"
        disabled={!searchResults.length}
        onclick={onNext}>›</SquareButton
      >
      <SquareButton
        label="Close PDF search"
        onclick={onClose}>×</SquareButton
      >
    </SearchField>
    {#if searching}<p>Searching…</p>{:else if selectedSearchResult >= 0}<p>
        {searchResults[selectedSearchResult]?.excerpt}
      </p>{/if}
  </section>
{/if}

<style>
  .pdf-search {
    position: fixed;
    z-index: 19;
    top: var(--floating-overlay-top);
    left: 0;
    right: 0;
    display: grid;
    gap: 6px;
    padding: 8px max(12px, env(safe-area-inset-right)) 8px
      max(12px, env(safe-area-inset-left));
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-bg);
  }

  .pdf-search :global(.pdf-search-form) {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .pdf-search :global(.pdf-search-form .search-input) {
    min-width: 90px;
    min-height: 34px;
    flex: 1;
    padding: 0 10px;
    border: 1px solid var(--color-border);
    color: inherit;
    background: var(--color-raised-bg);
    font: inherit;
  }

  .pdf-search :global(.pdf-search-form output) {
    min-width: 42px;
    text-align: center;
    font-size: 0.78rem;
  }

  .pdf-search p {
    margin: 0;
    overflow: hidden;
    color: var(--color-ink-muted);
    font-size: 0.78rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  :global(.pdf-search-form .search-input:focus-visible) {
    outline: 2px solid var(--color-border);
    outline-offset: 1px;
  }
</style>
