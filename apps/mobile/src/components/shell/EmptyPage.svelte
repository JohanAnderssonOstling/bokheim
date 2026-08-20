<script lang="ts">
  import PrimaryButton from "../shared/buttons/PrimaryButton.svelte";
  import SecondaryButton from "../shared/buttons/SecondaryButton.svelte";

  let {
    title = "",
    description = "",
    primaryLabel = "",
    secondaryLabel = "",
    onprimary = () => undefined,
    onsecondary = () => undefined,
  }: {
    title?: string;
    description?: string;
    primaryLabel?: string;
    secondaryLabel?: string;
    onprimary?: () => void | Promise<void>;
    onsecondary?: () => void | Promise<void>;
  } = $props();
</script>

<section class="empty-page">
  {#if title}<strong>{title}</strong>{/if}
  {#if description}<p>{description}</p>{/if}
  {#if primaryLabel || secondaryLabel}
    <div class="empty-page-actions">
      {#if primaryLabel}<PrimaryButton onclick={() => void onprimary()}
          >{primaryLabel}</PrimaryButton
        >{/if}
      {#if secondaryLabel}<SecondaryButton onclick={() => void onsecondary()}
          >{secondaryLabel}</SecondaryButton
        >{/if}
    </div>
  {/if}
</section>

<style>
  .empty-page {
    display: grid;
    width: 100%;
    min-height: 240px;
    place-content: center;
    justify-items: center;
    gap: 9px;
    padding: 18px;
    text-align: center;
  }
  strong {
    color: var(--color-ink-text);
    font: 600 1.3125rem/1.3 var(--font-interface-family);
  }
  p {
    max-width: 420px;
    margin: 0;
    color: var(--color-subtle-text);
    font-size: 0.8125rem;
    line-height: 1.5;
  }
  .empty-page-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 0.55rem;
    margin-top: 5px;
  }
</style>
