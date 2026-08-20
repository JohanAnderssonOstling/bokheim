<script lang="ts">
  import type { Snippet } from "svelte";

  let { title = undefined, children }: { title?: string; children: Snippet } = $props();
</script>

<fieldset>
  {#if title}<legend>{title}</legend>{/if}
  {@render children()}
</fieldset>

<style>
  fieldset {
    display: grid;
    gap: 0.55rem;
    margin: 0;
    padding: 0;
    border: 0;
  }

  legend {
    margin-bottom: 0.55rem;
    color: var(--color-subtle-text);
    font-size: 0.75rem;
    font-weight: 750;
  }
</style>
