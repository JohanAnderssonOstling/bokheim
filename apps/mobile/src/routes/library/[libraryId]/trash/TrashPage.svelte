<script lang="ts">
  import DangerButton from "../../../../components/shared/buttons/DangerButton.svelte";
  import SecondaryButton from "../../../../components/shared/buttons/SecondaryButton.svelte";
  import StatusState from "../../../../components/shared/StatusState.svelte";
  import type {
    LibraryTrashView,
    TrashedBook,
    TrashedFolder,
  } from "../../../../lib/bindings";

  let {
    contents,
    loading,
    busy,
    error,
    onretry,
    onrestorebook,
    onpurgebook,
    onrestorefolder,
    onpurgefolder,
    onempty,
  }: {
    contents: LibraryTrashView;
    loading: boolean;
    busy: boolean;
    error: string;
    onretry: () => void | Promise<void>;
    onrestorebook: (book: TrashedBook) => void | Promise<void>;
    onpurgebook: (book: TrashedBook) => void | Promise<void>;
    onrestorefolder: (folder: TrashedFolder) => void | Promise<void>;
    onpurgefolder: (folder: TrashedFolder) => void | Promise<void>;
    onempty: () => void | Promise<void>;
  } = $props();

  const empty = $derived(!contents.books.length && !contents.folders.length);

  function confirmPermanent(label: string): boolean {
    return window.confirm(`Delete “${label}” permanently? This cannot be undone.`);
  }
</script>

<section
  class="trash-page"
  aria-label="Trash"
>
  <header>
    <div>
      <h2>Trash</h2>
      <p>Deleted books and folders remain recoverable until permanently removed.</p>
    </div>
    {#if !empty}<DangerButton
        disabled={busy}
        onclick={() => window.confirm("Empty Trash? This cannot be undone.") && onempty()}
        >Empty Trash</DangerButton
      >{/if}
  </header>
  {#if loading}
    <StatusState
      variant="compact"
      message="Loading Trash…"
    />
  {:else if error}
    <StatusState
      kind="error"
      variant="compact"
      message={error}
      actionLabel="Retry"
      onaction={onretry}
    />
  {:else if empty}
    <StatusState
      variant="compact"
      title="Trash is empty"
      message="Deleted books and folders will appear here."
    />
  {:else}
    {#if contents.folders.length}
      <section class="group">
        <h3>Folders</h3>
        {#each contents.folders as folder (folder.id)}<div class="row">
            <div><strong>{folder.name}</strong><span>Folder</span></div>
            <div class="actions">
              <SecondaryButton
                disabled={busy}
                onclick={() => onrestorefolder(folder)}>Restore</SecondaryButton
              ><DangerButton
                disabled={busy}
                onclick={() => confirmPermanent(folder.name) && onpurgefolder(folder)}
                >Delete permanently</DangerButton
              >
            </div>
          </div>{/each}
      </section>
    {/if}
    {#if contents.books.length}
      <section class="group">
        <h3>Books</h3>
        {#each contents.books as book (book.content_hash)}<div class="row">
            <div>
              <strong>{book.title}</strong><span
                >{book.format ? book.format.toUpperCase() : "Book"}</span
              >
            </div>
            <div class="actions">
              <SecondaryButton
                disabled={busy}
                onclick={() => onrestorebook(book)}>Restore</SecondaryButton
              ><DangerButton
                disabled={busy}
                onclick={() => confirmPermanent(book.title) && onpurgebook(book)}
                >Delete permanently</DangerButton
              >
            </div>
          </div>{/each}
      </section>
    {/if}
  {/if}
</section>

<style>
  .trash-page {
    display: grid;
    width: min(100%, 64rem);
    margin: 0 auto;
    gap: 1.25rem;
  }
  header,
  .row,
  .actions {
    display: flex;
    align-items: center;
  }
  header {
    justify-content: space-between;
    gap: 1rem;
  }
  h2,
  h3,
  p {
    margin: 0;
  }
  h2 {
    font-size: 1.4rem;
  }
  h3 {
    padding: 0.75rem 1rem;
    font-size: 0.82rem;
    color: var(--color-subtle-text);
  }
  p,
  .row span {
    color: var(--color-subtle-text);
    font-size: 0.78rem;
  }
  .group {
    overflow: hidden;
    border: 1px solid var(--color-border);
    border-radius: 0;
    background: var(--color-raised-bg);
  }
  .row {
    justify-content: space-between;
    gap: 1rem;
    min-height: 4rem;
    padding: 0.7rem 1rem;
    border-top: 1px solid var(--color-border);
  }
  .row > div:first-child {
    display: grid;
    gap: 0.2rem;
    min-width: 0;
  }
  .row strong {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    gap: 0.5rem;
  }
  @media (max-width: 44.99rem) {
    header,
    .row {
      align-items: stretch;
      flex-direction: column;
    }
    .actions {
      display: grid;
      grid-template-columns: 1fr 1fr;
    }
  }
</style>
