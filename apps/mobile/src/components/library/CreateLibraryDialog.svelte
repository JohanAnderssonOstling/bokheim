<script lang="ts">
  import { modalFocus } from "../../lib/keyboard-navigation";

  let { busy = false, onclose, oncreate }: { busy?: boolean; onclose: () => void; oncreate: (name: string) => void | Promise<void> } = $props();
  let name = $state("");

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const value = name.trim();
    if (!value || busy) return;
    await oncreate(value);
  }
</script>

<button class="scrim" type="button" aria-label="Cancel library creation" onclick={onclose}></button>
<dialog open aria-modal="true" aria-labelledby="create-library-title" use:modalFocus={{ onClose: onclose, initialFocus: "input" }}>
  <header><h2 id="create-library-title">Create library</h2><button type="button" aria-label="Close" onclick={onclose}>×</button></header>
  <form onsubmit={submit}>
    <label><span>Library name</span><input required maxlength="255" bind:value={name} autocomplete="off" /></label>
    <div class="actions"><button class="secondary" type="button" onclick={onclose}>Cancel</button><button class="primary" type="submit" disabled={busy || !name.trim()}>{busy ? "Creating…" : "Create"}</button></div>
  </form>
</dialog>

<style>
  button, input { font: inherit; }
  .scrim { position: fixed; z-index: 90; inset: 0; border: 0; background: var(--color-overlay-scrim); }
  dialog { position: fixed; z-index: 91; inset: 50% auto auto 50%; width: min(28rem, calc(100vw - 2rem)); margin: 0; padding: 1rem; border: 1px solid var(--color-border); border-radius: 0; color: var(--color-ink-text); background: var(--color-raised-bg); transform: translate(-50%, -50%); }
  header, .actions { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  h2 { margin: 0; font: 600 1.15rem var(--font-interface-family); }
  header button { width: 2.5rem; height: 2.5rem; border: 0; color: var(--color-subtle-text); background: transparent; font-size: 1.5rem; }
  form, label { display: grid; gap: .75rem; }
  form { margin-top: 1rem; }
  label span { color: var(--color-subtle-text); font-size: .72rem; font-weight: 700; }
  input { min-height: 2.7rem; padding: .55rem .7rem; border: 1px solid var(--color-border); border-radius: 0; color: inherit; background: var(--color-raised-bg); }
  .actions { justify-content: end; }
  .actions button { min-height: 2.55rem; padding: .5rem .85rem; border-radius: 0; font-weight: 750; }
  .secondary { border: 1px solid var(--color-border); color: var(--color-subtle-text); background: transparent; }
  .primary { border: 1px solid var(--color-accent); color: var(--color-raised-bg); background: var(--color-accent); }
</style>
