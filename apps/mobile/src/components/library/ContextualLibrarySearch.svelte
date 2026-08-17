<script lang="ts">
  let {
    value = $bindable(),
    placeholder,
    oninput,
  }: {
    value: string;
    placeholder: string;
    oninput: () => void;
  } = $props();
</script>

<label class="search-field">
  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m21 21-4.35-4.35m2.35-5.15a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0"></path></svg>
  <input id="contextual-library-search" type="search" bind:value {oninput} {placeholder} aria-label={placeholder} autocomplete="off" />
</label>

<style>
  .search-field {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 9px;
    height: 44px;
    padding: 0 14px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
  }
  .search-field:focus-within { border-color: var(--color-accent); }
  svg { width: 18px; color: var(--color-accent); fill: none; stroke: currentColor; stroke-width: 1.8; }
  input { min-width: 0; border: 0; outline: 0; color: inherit; background: transparent; font: inherit; }
  @media (max-width: 719px) { .search-field { height: 48px; } }
</style>
