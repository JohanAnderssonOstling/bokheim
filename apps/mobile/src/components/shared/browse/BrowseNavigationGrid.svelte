<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<div class="navigation-grid">
  {@render children()}
</div>

<style>
  .navigation-grid {
    display: grid;
    grid-template-columns: repeat(
      auto-fill,
      minmax(min(100%, var(--hierarchy-cell-min-width)), 1fr)
    );
    gap: var(--hierarchy-cell-gap);
    margin-bottom: 18px;
  }
</style>
