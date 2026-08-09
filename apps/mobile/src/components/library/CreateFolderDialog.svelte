<script lang="ts">
  import { modalFocus } from "../../lib/keyboard-navigation";

  let {
    onclose,
    oncreate,
  }: {
    onclose: () => void;
    oncreate: (name: string) => Promise<void>;
  } = $props();

  let name = $state("");
  let pending = $state(false);
  let error = $state("");
  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const folderName = name.trim();
    if (!folderName || pending) return;

    pending = true;
    error = "";
    try {
      await oncreate(folderName);
      onclose();
    } catch (reason) {
      error = reason instanceof Error ? reason.message : String(reason);
    } finally {
      pending = false;
    }
  }
</script>

<button class="scrim" type="button" aria-label="Cancel folder creation" onclick={onclose}></button>
<div class="dialog" role="dialog" aria-modal="true" aria-labelledby="create-folder-title" use:modalFocus={{ onClose: onclose, initialFocus: "#folder-name" }}>
  <form onsubmit={submit}>
    <header>
      <h2 id="create-folder-title">New folder</h2>
      <button class="close" type="button" aria-label="Close" onclick={onclose}>×</button>
    </header>
    <label for="folder-name">Folder name</label>
    <input id="folder-name" bind:value={name} maxlength="255" autocomplete="off" />
    {#if error}<p role="alert">{error}</p>{/if}
    <footer>
      <button class="cancel" type="button" onclick={onclose}>Cancel</button>
      <button class="create" type="submit" disabled={!name.trim() || pending}>{pending ? "Creating…" : "Create"}</button>
    </footer>
  </form>
</div>

<style>
  button, input { font: inherit; }
  .scrim { position: fixed; z-index: 90; inset: 0; width: 100%; height: 100%; border: 0; background: var(--color-overlay-scrim); }
  .dialog { position: fixed; z-index: 91; right: 1rem; bottom: max(1rem, env(safe-area-inset-bottom)); left: 1rem; display: grid; gap: .65rem; padding: 1rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-lg); color: var(--color-ink-text); background: var(--color-white-surface); box-shadow: 0 1rem 3rem var(--color-box-shadow); }
  form { display: grid; gap: .65rem; }
  header { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 1.25rem; font-weight: 500; }
  .close { width: 2.5rem; height: 2.5rem; border: 0; color: var(--color-subtle-text); background: transparent; font-size: 1.5rem; }
  label { color: var(--color-subtle-text); font-size: .75rem; font-weight: 700; }
  input { width: 100%; min-height: 2.9rem; padding: .65rem .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: inherit; background: var(--color-card-bg); outline: none; }
  input:focus { border-color: var(--color-accent); box-shadow: 0 0 0 .18rem color-mix(in srgb, var(--color-accent) 14%, transparent); }
  p { margin: 0; color: var(--color-danger-text); font-size: .75rem; overflow-wrap: anywhere; }
  footer { display: flex; justify-content: flex-end; gap: .5rem; margin-top: .35rem; }
  footer button { min-height: 2.65rem; padding: .6rem 1rem; border-radius: .6rem; font-size: .8rem; font-weight: 750; }
  .cancel { border: 1px solid var(--color-border-color); color: var(--color-accent); background: transparent; }
  .create { border: 0; color: var(--color-white-surface); background: var(--color-accent); }
  .create:disabled { opacity: .55; }
  @media (min-width: 45rem) { .dialog { top: 50%; right: auto; bottom: auto; left: 50%; width: min(28rem, calc(100vw - 2rem)); transform: translate(-50%, -50%); } }
</style>
