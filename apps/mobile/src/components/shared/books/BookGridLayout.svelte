<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    detailed = false,
    variant = "library",
    children,
  }: {
    detailed?: boolean;
    variant?: "library" | "catalogue";
    children: Snippet;
  } = $props();
</script>

<div
  class="book-grid"
  class:library={variant === "library"}
  class:catalogue={variant === "catalogue"}
  class:detailed
>
  {@render children()}
</div>

<style>
  .book-grid {
    display: grid;
    grid-template-columns: repeat(
      auto-fit,
      minmax(
        min(var(--book-grid-min-width, var(--book-cover-width)), 100%),
        var(--book-grid-max-width, 1fr)
      )
    );
    gap: var(--book-grid-row-gap, 18px) var(--book-grid-column-gap, 14px);
  }
  .book-grid.library {
    --book-grid-min-width: var(--book-cover-width);
    --book-grid-max-width: var(--book-cover-width);
    --book-grid-column-gap: 28px;
    --book-grid-row-gap: calc(var(--book-grid-row-height) - var(--book-card-height));
    --book-grid-justify: space-between;
  }
  .book-grid.catalogue {
    --book-grid-min-width: min(
      max(
        var(--store-publication-min-width),
        calc(var(--book-cover-width) + 20rem + 48px)
      ),
      100%
    );
    --book-grid-max-width: 1fr;
    --book-grid-column-gap: 8px;
    --book-grid-row-gap: 8px;
  }
  .book-grid.detailed {
    grid-template-columns: minmax(0, 1fr);
    gap: 0;
  }
  .book-grid :global(.book-card) {
    content-visibility: auto;
    contain-intrinsic-size: var(--book-cover-width) var(--book-card-height);
  }
  .book-grid :global(.book-card:has(> .book-actions[open])) {
    content-visibility: visible;
  }
  .book-grid.detailed :global(.book-card) {
    width: 100%;
  }
  @media (min-width: 45rem) {
    .book-grid {
      row-gap: calc(var(--book-grid-row-height) - var(--book-card-height));
      justify-content: var(--book-grid-justify, start);
    }
    .book-grid.detailed {
      grid-template-columns: 1fr;
      row-gap: 0;
    }
  }
</style>
