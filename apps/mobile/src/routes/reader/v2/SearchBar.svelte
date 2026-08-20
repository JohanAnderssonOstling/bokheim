<!--
  Runs a search and steps through its matches. Holds only what is being typed;
  the matches themselves belong to the reader, which is what highlights them.

  Measurements match the v1 search bar. Its `--chrome` tokens are defined in the
  v1 reader's own stylesheet, so the values they alias are used directly.
-->
<script>
  import SquareButton from "../../../components/shared/buttons/SquareButton.svelte";

  let { count = 0, index = -1, onSearch, onCycle } = $props();

  let query = $state("");

  function submit(event) {
    event.preventDefault();
    onSearch(query);
  }
</script>

<section
  class="reader-search-bar"
  aria-label="Search book"
>
  <form
    class="search-form"
    onsubmit={submit}
  >
    <input
      class="search-input"
      type="search"
      placeholder="Search this book"
      aria-label="Search text"
      bind:value={query}
    />

    <span
      class="reader-match-count"
      aria-live="polite">{count ? `${Math.max(1, index + 1)}/${count}` : "0/0"}</span
    >

    <SquareButton
      label="Previous match"
      disabled={!count}
      onclick={() => onCycle(-1)}>‹</SquareButton
    >
    <SquareButton
      label="Next match"
      disabled={!count}
      onclick={() => onCycle(1)}>›</SquareButton
    >
  </form>
</section>

<style>
  .reader-search-bar {
    z-index: 7;
    flex: none;
    box-sizing: border-box;
    min-width: 0;
    min-height: calc(52px + env(safe-area-inset-bottom));
    padding: 8px 12px calc(8px + env(safe-area-inset-bottom));
    overflow-x: auto;
    background: var(--color-surface-bg);
    border-top: 1px solid var(--color-border);
  }

  .search-form {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: max-content;
  }

  .search-input {
    width: 260px;
    height: 36px;
    min-height: 36px;
    padding: 0 10px;
    background: var(--color-raised-bg);
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    font: inherit;
  }

  .reader-match-count {
    min-width: 42px;
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
  }
</style>
