<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    children,
    loading = false,
    error = "",
    ariaLabel = "Book content",
  }: {
    children: Snippet;
    loading?: boolean;
    error?: string;
    ariaLabel?: string;
  } = $props();
</script>

<section
  class="viewport"
  aria-label={ariaLabel}
>
  {@render children()}
  {#if loading}
    <div class="reader-message">Opening book…</div>
  {:else if error}
    <div class="reader-message error">
      <strong>This book could not be opened.</strong>
      <span>{error}</span>
    </div>
  {/if}
</section>

<style>
  /* This is the EPUB content surface, not the browser viewport. The class is
     intentionally retained because annotation placement and shell layout use
     it as their stable coordinate system. */
  .viewport {
    position: relative;
    min-height: 0;
    transition: margin 120ms ease;
  }

  .reader-message {
    position: absolute;
    inset: 0;
    display: grid;
    place-content: center;
    gap: 0.6rem;
    padding: 2rem;
    text-align: center;
    background: inherit;
  }

  .reader-message.error span {
    max-width: 28rem;
    font-size: 0.85rem;
    opacity: 0.7;
  }

  :global(.view-host),
  :global(.foliate-view) {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
