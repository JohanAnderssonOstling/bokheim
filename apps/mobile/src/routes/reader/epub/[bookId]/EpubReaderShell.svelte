<script lang="ts">
  import type { Snippet } from "svelte";
  import { READER_SETTINGS_PRESENTATION_CONTRACT } from "../../../../lib/shared/types";
  import "../../../../components/shared/readers/chrome/reader-surface.css";

  let {
    children,
    hiddenChrome = false,
    hasSidebar = false,
    searchActive = false,
    onPointerMove,
    onPointerDown,
  }: {
    children: Snippet;
    hiddenChrome?: boolean;
    hasSidebar?: boolean;
    searchActive?: boolean;
    onPointerMove?: (event: PointerEvent) => void;
    onPointerDown?: (event: PointerEvent) => void;
  } = $props();

  const settingsStyle = `--reader-settings-max-width: ${READER_SETTINGS_PRESENTATION_CONTRACT.max_width_px}px; --reader-settings-max-height: ${READER_SETTINGS_PRESENTATION_CONTRACT.max_height_px}px; --reader-settings-padding: ${READER_SETTINGS_PRESENTATION_CONTRACT.padding_px}px; --reader-settings-gap: ${READER_SETTINGS_PRESENTATION_CONTRACT.gap_px}px; --reader-settings-min-column-width: ${READER_SETTINGS_PRESENTATION_CONTRACT.minimum_column_width_px}px;`;
</script>

<main
  class="reader-shell epub-reader-shell"
  class:hidden-chrome={hiddenChrome}
  class:has-sidebar={hasSidebar}
  class:search-active={searchActive}
  style={settingsStyle}
  onpointermove={onPointerMove}
  onpointerdown={onPointerDown}
  data-reader-settings-presentation={READER_SETTINGS_PRESENTATION_CONTRACT.presentation}
>
  {@render children()}
</main>

<style>
  .epub-reader-shell {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    transition: grid-template-rows 160ms ease;
  }

  .epub-reader-shell.hidden-chrome {
    grid-template-rows: 0 minmax(0, 1fr);
  }

  @media (prefers-reduced-motion: reduce) {
    .epub-reader-shell {
      transition: none;
    }
  }

  :global(.epub-reader-shell.has-sidebar .viewport) {
    margin-left: min(380px, 40vw);
  }

  :global(.epub-reader-shell.search-active .viewport) {
    margin-bottom: calc(52px + env(safe-area-inset-bottom));
  }

  @media (max-width: 720px) {
    :global(.epub-reader-shell.has-sidebar .viewport) {
      margin-left: 0;
    }
  }
</style>
