<script lang="ts">
  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

<header class="topbar">
  {@render children()}
</header>

<style>
  .topbar {
    position: sticky;
    z-index: 20;
    top: 0;
    display: flex;
    flex: none;
    align-items: center;
    gap: 0.55rem;
    margin-right: calc(-1 * var(--content-inset-inline-end, 0px));
    margin-left: calc(-1 * var(--content-inset-inline-start, 0px));
    padding: calc(0.55rem + env(safe-area-inset-top))
      max(var(--page-gutter, 1rem), env(safe-area-inset-right))
      var(--content-section-gap, 0.55rem)
      max(var(--page-gutter, 1rem), env(safe-area-inset-left));
    background: color-mix(in srgb, var(--color-surface-bg) 96%, transparent);
    backdrop-filter: blur(18px);
  }

  @media (min-width: 45rem) {
    .topbar {
      gap: 0.75rem;
    }
  }
</style>
