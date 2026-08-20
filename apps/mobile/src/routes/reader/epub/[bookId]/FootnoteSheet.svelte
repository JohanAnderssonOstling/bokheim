<script lang="ts">
  import PanelHeader from "../../../../components/shared/readers/panels/PanelHeader.svelte";

  let {
    open,
    label,
    onClose,
    onHostMount,
  }: {
    open: boolean;
    label: string;
    onClose: () => void;
    onHostMount: (host: HTMLDivElement) => void;
  } = $props();

  let footnoteHost = $state<HTMLDivElement>();

  $effect(() => {
    if (open && footnoteHost) onHostMount(footnoteHost);
  });
</script>

{#if open}
  <dialog
    open
    class="modal-backdrop"
    aria-label={label}
    onclick={(event) => {
      if (event.target === event.currentTarget) onClose();
    }}
  >
    <section class="footnote-sheet">
      <PanelHeader
        title={label}
        closeLabel="Close note"
        variant="sheet"
        onclose={onClose}
      />
      <div
        class="footnote-host"
        bind:this={footnoteHost}
      ></div>
    </section>
  </dialog>
{/if}

<style>
  .modal-backdrop {
    position: absolute;
    z-index: 10;
    inset: 0;
    width: 100%;
    max-width: none;
    height: 100%;
    max-height: none;
    margin: 0;
    border: 0;
    display: grid;
    align-items: end;
    padding: max(1rem, env(safe-area-inset-top)) max(0.75rem, env(safe-area-inset-right))
      calc(0.75rem + env(safe-area-inset-bottom)) max(0.75rem, env(safe-area-inset-left));
    box-sizing: border-box;
    background: var(--color-overlay-scrim);
  }

  .footnote-sheet {
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    width: min(42rem, 100%);
    max-height: min(65dvh, 34rem);
    margin: 0 auto;
    overflow: hidden;
    border: 1px solid var(--chrome-strong);
    border-radius: 0;
    color: var(--ink);
    background: var(--chrome);
    box-shadow: 0 1.5rem 4rem color-mix(in srgb, var(--color-ink-text) 32%, transparent);
  }

  .footnote-host {
    min-height: 8rem;
    height: min(48dvh, 26rem);
  }

  :global(.footnote-view) {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
