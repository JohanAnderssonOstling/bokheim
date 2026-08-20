<script lang="ts">
  import type { Snippet } from "svelte";
  import "../../../../../components/shared/readers/chrome/reader-surface.css";

  let {
    children,
    hiddenChrome = false,
    onPointerDown,
    onPointerMove,
    onPointerUp,
  }: {
    children: Snippet;
    hiddenChrome?: boolean;
    onPointerDown?: (event: PointerEvent) => void;
    onPointerMove?: (event: PointerEvent) => void;
    onPointerUp?: (event: PointerEvent) => void;
  } = $props();
</script>

<main
  class="reader-shell pdf-reader-shell"
  class:hidden-chrome={hiddenChrome}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
>
  {@render children()}
</main>

<style>
  .pdf-reader-shell {
    --reader-topbar-height: 3.625rem;
    --reader-topbar-offset: calc(var(--reader-topbar-height) + env(safe-area-inset-top));
    --reader-topbar-top: var(--reader-topbar-offset);
    --reader-panel-top: var(--reader-topbar-offset);
    --floating-overlay-top: var(--reader-topbar-offset);
    --reader-topbar-shift: 0px;
    display: block;
    position: relative;
    overflow: hidden;
    color: var(--color-ink-text);
    background: var(--color-surface-bg);
  }

  .pdf-reader-shell.hidden-chrome {
    --reader-topbar-top: 0px;
    --floating-overlay-top: 0px;
    --reader-panel-top: 0px;
    --reader-topbar-shift: calc(-1 * var(--reader-topbar-offset));
  }

  :global(.pdf-viewport) {
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    overflow: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    background: color-mix(
      in srgb,
      var(--color-page-bg, var(--color-surface-bg)) 84%,
      var(--color-ink-text)
    );
  }

  :global(.page-stream) {
    display: grid;
    grid-template-columns: repeat(var(--pdf-columns), max-content);
    justify-content: center;
    align-items: start;
    column-gap: 1px;
    min-width: min-content;
    width: 100%;
    padding: 0;
    box-sizing: border-box;
  }

  :global(.page-stream.fit-page) {
    min-height: 100%;
    justify-content: flex-start;
  }
</style>
