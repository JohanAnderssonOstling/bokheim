<script lang="ts">
  import ModalShell from "./ModalShell.svelte";
  import FormButton from "../buttons/FormButton.svelte";
  import SecondaryButton from "../buttons/SecondaryButton.svelte";

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

<ModalShell
  title="New folder"
  titleId="create-folder-title"
  initialFocus="#folder-name"
  {onclose}
>
  <form onsubmit={submit}>
    <label for="folder-name">Folder name</label>
    <input
      id="folder-name"
      bind:value={name}
      maxlength="255"
      autocomplete="off"
    />
    {#if error}<p role="alert">{error}</p>{/if}
    <footer>
      <SecondaryButton onclick={onclose}>Cancel</SecondaryButton>
      <FormButton disabled={!name.trim() || pending}
        >{pending ? "Creating…" : "Create"}</FormButton
      >
    </footer>
  </form>
</ModalShell>

<style>
  input {
    font: inherit;
  }
  form {
    display: grid;
    gap: 0.65rem;
  }
  label {
    color: var(--color-subtle-text);
    font-size: 0.75rem;
    font-weight: 700;
  }
  input {
    width: 100%;
    min-height: 2.9rem;
    padding: 0.65rem 0.75rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
    outline: none;
  }
  input:focus {
    border-color: var(--color-accent);
    box-shadow: 0 0 0 0.18rem color-mix(in srgb, var(--color-accent) 14%, transparent);
  }
  p {
    margin: 0;
    color: var(--color-danger-text);
    font-size: 0.75rem;
    overflow-wrap: anywhere;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 0.35rem;
  }
</style>
