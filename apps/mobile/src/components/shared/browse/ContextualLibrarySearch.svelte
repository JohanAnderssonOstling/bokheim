<script lang="ts">
  import SearchField from "../SearchField.svelte";
  import { consumeSearchFocus, onSearchFocus } from "../../../lib/app/search-focus";

  let field = $state<{ focus: () => void } | undefined>();

  // The shortcut can arrive before this field exists, because reaching a page
  // that has one may take a navigation. Claiming the pending request on mount
  // covers that case; the subscription covers the field already being here.
  $effect(() => {
    if (consumeSearchFocus()) field?.focus();
    return onSearchFocus(() => field?.focus());
  });

  let {
    value = $bindable(),
    placeholder,
    oninput,
  }: {
    value: string;
    placeholder: string;
    oninput: (value: string) => void;
  } = $props();
</script>

<SearchField
  bind:this={field}
  class="contextual-library-search"
  bind:value
  {oninput}
  id="contextual-library-search"
  {placeholder}
  ariaLabel={placeholder}
/>

<style>
  :global(.contextual-library-search) {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 9px;
    height: var(--layout-topbar-action-height);
    padding: 0 14px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: transparent;
  }
  :global(.contextual-library-search:focus-within) {
    border-color: var(--color-accent);
  }
  :global(.contextual-library-search .file-icon) {
    color: var(--color-accent);
  }
  :global(.contextual-library-search .search-input) {
    min-width: 0;
    border: 0;
    outline: 0;
    color: inherit;
    background: transparent;
    font: inherit;
  }
  :global(.contextual-library-search .search-input::placeholder) {
    color: transparent;
  }
  @media (max-width: 719px) {
    :global(.contextual-library-search) {
      height: 48px;
    }
    :global(.contextual-library-search .search-input::placeholder) {
      color: var(--color-subtle-text);
    }
  }
</style>
