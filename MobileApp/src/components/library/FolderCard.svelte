<script lang="ts">
  import type { FolderDownloadStatus, LibraryDirectory } from "../../lib/types";
  import DownloadIndicator from "./DownloadIndicator.svelte";

  let {
    folder,
    status,
    active = false,
    onopen,
    ondownload,
  }: {
    folder: LibraryDirectory;
    status?: FolderDownloadStatus;
    active?: boolean;
    onopen: (folder: LibraryDirectory) => void | Promise<void>;
    ondownload: (folder: LibraryDirectory) => void | Promise<void>;
  } = $props();

  const total = $derived(status?.total_books ?? folder.book_count);
  const downloaded = $derived(status?.downloaded_books ?? 0);
  const progress = $derived(total ? downloaded / total : 0);
  const complete = $derived(total > 0 && downloaded >= total);
</script>

<div class="folder-card">
  <button class="folder-open" type="button" title={folder.name} onclick={() => onopen(folder)}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6.5h7l2 2h9v10.5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path></svg>
    <span>{folder.name}</span>
  </button>
  {#if total > 0}
    <div class="download-action">
      <DownloadIndicator
        {progress}
        {active}
        {complete}
        label={complete ? `${folder.name} downloaded` : `Download ${folder.name}, ${downloaded} of ${total} books downloaded`}
        onclick={(event) => { event.stopPropagation(); return ondownload(folder); }}
      />
    </div>
  {/if}
</div>

<style>
  button { font: inherit; -webkit-tap-highlight-color: transparent; }
  .folder-card { position: relative; width: max-content; max-width: 100%; min-width: 0; }
  .folder-open { display: grid; width: 100%; min-width: 0; grid-template-columns: 1.35rem minmax(0, 1fr); gap: .65rem; align-items: center; padding: .72rem 2.7rem .72rem .75rem; border: 1px solid var(--color-border-color); border-radius: var(--radius-sm); color: inherit; background: var(--color-white-surface); text-align: left; cursor: pointer; }
  .folder-open:hover { background: var(--color-hover-bg); }
  .folder-open:active { transform: translateY(1px); }
  .folder-open svg { width: 1.35rem; height: 1.35rem; fill: none; stroke: var(--color-accent); stroke-linecap: round; stroke-linejoin: round; stroke-width: 1.7; }
  .folder-open span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .download-action { position: absolute; top: 50%; right: .55rem; transform: translateY(-50%); }
</style>
