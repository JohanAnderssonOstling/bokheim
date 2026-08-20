<script lang="ts">
  import SquareButton from "../../../../../components/shared/buttons/SquareButton.svelte";
  import PanelHeader from "../../../../../components/shared/readers/panels/PanelHeader.svelte";
  import SecondaryButton from "../../../../../components/shared/buttons/SecondaryButton.svelte";

  let {
    settingsOpen,
    fitMode,
    zoom,
    resolveLayout,
    widthFitZoom,
    relayout,
    adjustPageColumns,
    adjustZoom,
    minZoom,
    maxColumns,
    onClose,
  }: {
    settingsOpen: boolean;
    fitMode: "width" | "zoom" | "height";
    zoom: number;
    resolveLayout: () => {
      columns: number;
      columnWidth: number;
      zoom: number;
    };
    widthFitZoom: (columns: number) => number;
    relayout: (mode: "width" | "zoom" | "height", zoom: number) => Promise<void> | void;
    adjustPageColumns: (delta: number) => void;
    adjustZoom: (delta: number) => void;
    minZoom: number;
    maxColumns: number;
    onClose: () => void;
  } = $props();
</script>

{#if settingsOpen}
  <section
    class="pdf-panel settings-panel"
    aria-label="PDF appearance settings"
  >
    <PanelHeader
      title="Page layout"
      closeLabel="Close PDF settings"
      variant="bordered"
      onclose={onClose}
    />

    <div class="setting-row">
      <span>Fit</span>
      <div class="choice">
        <SecondaryButton
          active={fitMode === "width"}
          ariaPressed={fitMode === "width"}
          onclick={() => relayout("width", 1)}>Fit width</SecondaryButton
        ><SecondaryButton
          active={fitMode === "zoom"}
          ariaPressed={fitMode === "zoom"}
          onclick={() => relayout("zoom", resolveLayout().zoom)}>Zoom</SecondaryButton
        ><SecondaryButton
          active={fitMode === "height"}
          ariaPressed={fitMode === "height"}
          onclick={() => relayout("height", zoom)}>Fit height</SecondaryButton
        >
      </div>
    </div>
    {#if fitMode === "width"}
      <div class="setting-row">
        <span>Pages</span>
        <div class="stepper">
          <SquareButton
            label="Decrease pages"
            disabled={resolveLayout().columns <= 1}
            onclick={() => adjustPageColumns(-1)}>−</SquareButton
          ><output>{resolveLayout().columns}</output><SquareButton
            label="Increase pages"
            disabled={resolveLayout().columns >= maxColumns}
            onclick={() => adjustPageColumns(1)}>+</SquareButton
          >
        </div>
      </div>
    {:else if fitMode === "zoom"}
      <div class="setting-row">
        <span>Zoom</span>
        <div class="stepper">
          <SquareButton
            label="Decrease zoom"
            disabled={resolveLayout().zoom <= minZoom}
            onclick={() => adjustZoom(-0.1)}>−</SquareButton
          ><output>{Math.round(resolveLayout().zoom * 100)}%</output><SquareButton
            label="Increase zoom"
            disabled={resolveLayout().zoom >= widthFitZoom(1)}
            onclick={() => adjustZoom(0.1)}>+</SquareButton
          >
        </div>
      </div>
    {/if}
  </section>
{/if}

<style>
  .pdf-panel {
    position: fixed;
    z-index: 20;
    top: var(--floating-overlay-top);
    max-height: min(620px, calc(100dvh - 72px - env(safe-area-inset-top)));
    overflow: auto;
    box-sizing: border-box;
    border: 1px solid var(--color-border);
    background: var(--color-surface-bg);
    box-shadow: 0 12px 32px color-mix(in srgb, black 24%, transparent);
  }

  .settings-panel {
    right: max(12px, env(safe-area-inset-right));
    width: min(400px, calc(100vw - 24px));
    padding-bottom: 8px;
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    min-height: 52px;
    padding: 0 14px;
    border-bottom: 1px solid var(--color-border);
  }

  .setting-row > span {
    font-weight: 650;
  }

  .stepper,
  .choice {
    display: flex;
    align-items: center;
  }

  .stepper output {
    min-width: 58px;
    text-align: center;
    font-size: 0.8rem;
    font-variant-numeric: tabular-nums;
  }

  .choice :global(.base-button) {
    min-width: 88px;
  }

  @media (max-width: 520px) {
    .choice :global(.base-button) {
      min-width: 78px;
      padding-inline: 6px;
    }
  }
</style>
