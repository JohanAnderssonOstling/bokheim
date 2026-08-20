<script lang="ts">
  import SquareButton from "../../../../components/shared/buttons/SquareButton.svelte";
  import SearchField from "../../../../components/shared/SearchField.svelte";

  let {
    query,
    searchResultsLength,
    selectedSearchIndex,
    searchError,
    onQueryChange,
    onSubmitSearch,
    onCycleSearch,
  }: {
    query: string;
    searchResultsLength: number;
    selectedSearchIndex: number;
    searchError: string;
    onQueryChange: (query: string) => void;
    onSubmitSearch: (event: SubmitEvent) => void | Promise<void>;
    onCycleSearch: (offset: number) => void;
  } = $props();
</script>

<section
  class="reader-search-bar"
  aria-label="Search book"
>
  <SearchField
    class="search-form"
    showIcon={false}
    value={query}
    placeholder="Search this book"
    ariaLabel="Search text"
    oninput={onQueryChange}
    onsubmit={(event) => onSubmitSearch(event)}
  >
    <span
      class="reader-match-count"
      aria-live="polite"
      >{searchResultsLength
        ? `${Math.max(1, selectedSearchIndex + 1)}/${searchResultsLength}`
        : "0/0"}
    </span>
    <SquareButton
      label="Previous match"
      onclick={() => onCycleSearch(-1)}
    >
      ‹
    </SquareButton>
    <SquareButton
      label="Next match"
      onclick={() => onCycleSearch(1)}>›</SquareButton
    >
  </SearchField>
  {#if searchError}<span class="search-inline-error">{searchError}</span>{/if}
</section>

<style>
  .reader-search-bar {
    position: absolute;
    z-index: 7;
    right: 0;
    bottom: 0;
    left: 0;
    min-width: 0;
    min-height: calc(52px + env(safe-area-inset-bottom));
    padding: 8px 12px calc(8px + env(safe-area-inset-bottom));
    box-sizing: border-box;
    overflow-x: auto;
    border-top: 1px solid var(--chrome-strong);
    background: var(--chrome);
  }

  .reader-search-bar :global(.search-form) {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: max-content;
  }

  .reader-search-bar :global(.search-form .search-input) {
    width: 260px;
    height: 36px;
    min-height: 36px;
    padding: 0 10px;
    border: 1px solid var(--chrome-strong);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
    font: inherit;
  }

  .reader-match-count {
    min-width: 42px;
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
  }

  .search-inline-error {
    margin-left: 8px;
    color: var(--color-danger-text);
    font-size: 0.75rem;
  }
</style>
