<script lang="ts">
  import type { LibraryFolderDestination } from "../../lib/bindings.ts";
  import { modalFocus } from "../../lib/keyboard-navigation";

  let {
    title,
    actionLabel,
    destinations,
    initialId,
    onclose,
    onconfirm,
  }: {
    title: string;
    actionLabel: string;
    destinations: LibraryFolderDestination[];
    initialId: string;
    onclose: () => void;
    onconfirm: (destinationId: string) => Promise<void>;
  } = $props();

  let selectedId = $state("");
  let pending = $state(false);
  let error = $state("");

  $effect(() => {
    selectedId = initialId;
  });

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!selectedId || pending) return;
    pending = true;
    error = "";
    try {
      await onconfirm(selectedId);
      onclose();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      pending = false;
    }
  }
</script>

<button class="scrim" type="button" aria-label="Cancel folder selection" onclick={onclose}></button>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="folder-picker-title" use:modalFocus={{ onClose: onclose, initialFocus: "#folder-destination" }}>
  <form onsubmit={submit}>
    <header><h2 id="folder-picker-title">{title}</h2><button class="close" type="button" aria-label="Close" onclick={onclose}>×</button></header>
    <label for="folder-destination">Destination folder</label>
    <select id="folder-destination" bind:value={selectedId}>
      {#each destinations as destination}<option value={destination.id}>{destination.path}</option>{/each}
    </select>
    {#if error}<p role="alert">{error}</p>{/if}
    <footer>
      <button class="cancel" type="button" onclick={onclose}>Cancel</button>
      <button class="confirm" type="submit" disabled={!selectedId || pending}>{pending ? `${actionLabel}…` : actionLabel}</button>
    </footer>
  </form>
</div>

<style>
  button, select { font: inherit; }
  .scrim { position: fixed; z-index: 100; inset: 0; width: 100%; height: 100%; border: 0; background: var(--color-overlay-scrim); }
  .dialog { position: fixed; z-index: 101; right: 1rem; bottom: max(1rem, env(safe-area-inset-bottom)); left: 1rem; padding: 1rem; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-ink-text); background: var(--color-raised-bg); box-shadow: 0 1rem 3rem var(--color-box-shadow); }
  form { display: grid; gap: .75rem; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 1rem; }
  h2 { margin: 0; font-size: 1.15rem; font-weight: 600; }
  .close { width: 2.5rem; height: 2.5rem; border: 0; color: var(--color-subtle-text); background: transparent; font-size: 1.5rem; }
  label { color: var(--color-subtle-text); font-size: .75rem; font-weight: 700; }
  select { width: 100%; min-height: 3rem; padding: .65rem .75rem; border: 1px solid var(--color-border); border-radius: 0; color: inherit; background: var(--color-raised-bg); }
  p { margin: 0; color: var(--color-danger-text); font-size: .75rem; overflow-wrap: anywhere; }
  footer { display: flex; justify-content: flex-end; gap: .5rem; margin-top: .25rem; }
  footer button { min-height: 2.65rem; padding: .6rem 1rem; border-radius: 0; font-size: .8rem; font-weight: 750; }
  .cancel { border: 1px solid var(--color-border); color: var(--color-accent); background: transparent; }
  .confirm { border: 0; color: var(--color-raised-bg); background: var(--color-accent); }
  .confirm:disabled { opacity: .55; }
  @media (min-width: 45rem) { .dialog { top: 50%; right: auto; bottom: auto; left: 50%; width: min(32rem, calc(100vw - 2rem)); transform: translate(-50%, -50%); } }
</style>
