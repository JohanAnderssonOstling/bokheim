<script lang="ts">
  import ModalShell from "../../../../components/shared/dialogs/ModalShell.svelte";

  let { imageSrc, onClose }: { imageSrc: string; onClose: () => void } = $props();
</script>

{#if imageSrc}
  <ModalShell
    title="Image"
    titleId="epub-image-viewer-title"
    initialFocus="#epub-image-download"
    wide
    onclose={onClose}
  >
    <section class="image-viewer">
      <img
        src={imageSrc}
        alt="Expanded book illustration"
      />
      <a
        id="epub-image-download"
        href={imageSrc}
        download>Save image</a
      >
    </section>
  </ModalShell>
{/if}

<style>
  .image-viewer {
    display: grid;
    grid-template-rows: minmax(0, 1fr) auto;
    gap: 0.75rem;
    height: min(72dvh, 48rem);
  }

  .image-viewer img {
    width: 100%;
    height: 100%;
    min-height: 0;
    object-fit: contain;
    background: var(--color-surface-bg);
  }

  .image-viewer a {
    justify-self: end;
    color: var(--color-accent);
  }
</style>
