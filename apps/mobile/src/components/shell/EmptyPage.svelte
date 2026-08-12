<script lang="ts">
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
    <div class="actions">
      {#if primaryLabel}<button type="button" onclick={() => void onprimary()}>{primaryLabel}</button>{/if}
      {#if secondaryLabel}<button class="secondary" type="button" onclick={() => void onsecondary()}>{secondaryLabel}</button>{/if}
    </div>
  {/if}
</section>

<style>
  .empty-page { display: grid; width: 100%; min-height: 240px; place-content: center; justify-items: center; gap: 9px; padding: 18px; text-align: center; }
  strong { color: var(--color-ink-text); font: 600 19px/1.3 var(--font-interface-family); }
  p { max-width: 420px; margin: 0; color: var(--color-subtle-text); font-size: 13px; line-height: 1.5; }
  .actions { display: flex; flex-wrap: wrap; justify-content: center; gap: 10px; margin-top: 5px; }
  button { min-height: 40px; padding: 0 14px; border: 1px solid var(--color-accent); border-radius: var(--radius-sm); color: var(--color-raised-bg); background: var(--color-accent); font: 750 13px var(--font-interface-family); cursor: pointer; }
  button.secondary { color: var(--color-accent); background: transparent; }
</style>
