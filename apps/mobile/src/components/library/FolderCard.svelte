<script lang="ts">
  import type { FolderDownloadStatus, LibraryDirectory } from "../../lib/types";
  import { isWebApp } from "../../lib/platform";
  import DownloadIndicator from "./DownloadIndicator.svelte";
  import FolderNavigationCard from "./FolderNavigationCard.svelte";

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
  <FolderNavigationCard name={folder.name} reserveEnd={!isWebApp && total > 0} onclick={() => { if (!isWebApp && total > 0 && !complete) void ondownload(folder); return onopen(folder); }} />
  {#if !isWebApp && total > 0}
    <div class="download-action">
      <DownloadIndicator
        {progress}
        {active}
        {complete}
        label={complete ? `${folder.name} downloaded` : `Download ${folder.name}, ${downloaded} of ${total} books downloaded`}
      />
    </div>
  {/if}
</div>

<style>
  .folder-card { position: relative; width: max-content; max-width: 100%; min-width: 0; }
  .download-action { position: absolute; top: 50%; right: .55rem; transform: translateY(-50%); }
</style>
