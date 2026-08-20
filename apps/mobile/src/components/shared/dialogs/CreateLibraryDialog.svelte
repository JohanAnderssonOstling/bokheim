<script lang="ts">
  import ModalShell from "./ModalShell.svelte";
  import FormButton from "../buttons/FormButton.svelte";
  import SecondaryButton from "../buttons/SecondaryButton.svelte";

  let {
    busy = false,
    onclose,
    oncreate,
  }: {
    busy?: boolean;
    onclose: () => void;
    oncreate: (name: string) => void | Promise<void>;
  } = $props();
  let name = $state("");

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    const value = name.trim();
    if (!value || busy) return;
    await oncreate(value);
  }
</script>

<ModalShell
  title="Create library"
  titleId="create-library-title"
  initialFocus="input"
  {onclose}
>
  <form onsubmit={submit}>
    <label
      ><span>Library name</span><input
        required
        maxlength="255"
        bind:value={name}
        autocomplete="off"
      /></label
    >
    <div class="dialog-actions">
      <SecondaryButton onclick={onclose}>Cancel</SecondaryButton><FormButton
        disabled={busy || !name.trim()}>{busy ? "Creating…" : "Create"}</FormButton
      >
    </div>
  </form>
</ModalShell>

<style>
  input {
    font: inherit;
  }
  form,
  label {
    display: grid;
    gap: 0.75rem;
  }
  form {
    margin-top: 1rem;
  }
  .dialog-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.55rem;
  }
  label span {
    color: var(--color-subtle-text);
    font-size: 0.72rem;
    font-weight: 700;
  }
  input {
    min-height: 2.7rem;
    padding: 0.55rem 0.7rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
  }
</style>
