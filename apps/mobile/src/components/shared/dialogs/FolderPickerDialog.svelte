<script lang="ts">
  import type { LibraryFolderDestination } from "../../../lib/bindings.ts";
  import ModalShell from "./ModalShell.svelte";
  import FormButton from "../buttons/FormButton.svelte";
  import SecondaryButton from "../buttons/SecondaryButton.svelte";

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

<ModalShell
  {title}
  titleId="folder-picker-title"
  initialFocus="#folder-destination"
  {onclose}
>
  <form onsubmit={submit}>
    <label for="folder-destination">Destination folder</label>
    <select
      id="folder-destination"
      bind:value={selectedId}
    >
      {#each destinations as destination}<option value={destination.id}
          >{destination.path}</option
        >{/each}
    </select>
    {#if error}<p role="alert">{error}</p>{/if}
    <footer>
      <SecondaryButton onclick={onclose}>Cancel</SecondaryButton>
      <FormButton disabled={!selectedId || pending}
        >{pending ? `${actionLabel}…` : actionLabel}</FormButton
      >
    </footer>
  </form>
</ModalShell>

<style>
  select {
    font: inherit;
  }
  form {
    display: grid;
    gap: 0.75rem;
  }
  label {
    color: var(--color-subtle-text);
    font-size: 0.75rem;
    font-weight: 700;
  }
  select {
    width: 100%;
    min-height: 3rem;
    padding: 0.65rem 0.75rem;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
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
    margin-top: 0.25rem;
  }
</style>
