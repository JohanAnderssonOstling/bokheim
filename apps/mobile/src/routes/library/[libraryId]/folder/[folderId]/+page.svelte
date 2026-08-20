<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import BookGrid from "../../../../../components/shared/books/BookGrid.svelte";
  import CreateFolderDialog from "../../../../../components/shared/dialogs/CreateFolderDialog.svelte";
  import FolderPickerDialog from "../../../../../components/shared/dialogs/FolderPickerDialog.svelte";
  import StatusState from "../../../../../components/shared/StatusState.svelte";
  import FoldersPage from "./FoldersPage.svelte";
  import { createFolderRoute } from "./folder-route";
  import {
    onBookDownloadStatus,
    onFolderDownloadStatus,
    onLibraryRefresh,
  } from "../../../../../lib/app/library-events";

  const folders = createFolderRoute((path) => goto(path));
  $effect(
    () =>
      void folders.load(page.params.libraryId ?? "", page.params.folderId ?? "__root__"),
  );
  $effect(() =>
    onLibraryRefresh(
      () =>
        void folders.load(
          page.params.libraryId ?? "",
          page.params.folderId ?? "__root__",
          true,
        ),
    ),
  );
  $effect(() =>
    onBookDownloadStatus((change) =>
      folders.setDownloadState(change.contentHash, change.state),
    ),
  );
  $effect(() => onFolderDownloadStatus(folders.setFolderDownloadStatus));
</script>

<svelte:head><title>Folder · Bokheim</title></svelte:head>

{#snippet searchResults()}
  <div
    class="scoped-search-results"
    aria-live="polite"
  >
    {#if $folders.searchPending}<div class="inline-empty">Searching…</div>
    {:else if $folders.searchCompletedQuery}
      {#if $folders.titleSearchResults.length}<h2>Title matches</h2>
        <BookGrid
          libraryId={$folders.libraryId}
          books={$folders.titleSearchResults}
          detailed={$folders.detailed}
          emptyMessage=""
          openingBookId=""
          downloadStates={$folders.downloadStates}
          onopen={folders.open}
          ondownload={folders.download}
          onremove={folders.remove}
          ontrash={folders.trash}
        />{/if}
      {#if $folders.authorSearchResults.length}<h2>Author matches</h2>
        <BookGrid
          libraryId={$folders.libraryId}
          books={$folders.authorSearchResults}
          detailed={$folders.detailed}
          emptyMessage=""
          openingBookId=""
          downloadStates={$folders.downloadStates}
          onopen={folders.open}
          ondownload={folders.download}
          onremove={folders.remove}
          ontrash={folders.trash}
        />{/if}
      {#if !$folders.titleSearchResults.length && !$folders.authorSearchResults.length}<div
          class="inline-empty"
        >
          No matching books
        </div>{/if}
    {/if}
  </div>
{/snippet}

{#if $folders.loading}
  <StatusState message="Loading folder" />
{:else}
  <FoldersPage
    libraryId={$folders.libraryId}
    folders={$folders.content.children}
    books={$folders.content.books}
    folderPath={$folders.path}
    fileType={$folders.fileType}
    query={$folders.query}
    detailed={$folders.detailed}
    openingBookId=""
    downloadStates={$folders.downloadStates}
    folderDownloadStatuses={$folders.folderDownloadStatuses}
    activeFolderDownloads={$folders.activeFolderDownloads}
    {searchResults}
    onsearch={folders.scheduleSearch}
    onquerychange={folders.setQuery}
    onfiletype={folders.setFileType}
    onopenfolder={folders.openFolder}
    ondownloadfolder={folders.downloadFolder}
    ontrashfolder={folders.trashFolder}
    onrenamefolder={folders.rename}
    onmovefolder={folders.moveFolder}
    onopen={folders.open}
    ondownload={folders.download}
    onremove={folders.remove}
    ontrash={folders.trash}
    oncopybook={folders.copy}
    onmovebook={folders.move}
    onremovebookfromfolder={folders.removeFromFolder}
    onadd={folders.add}
    onnewfolder={folders.showCreateFolder}
    addingBook={$folders.addingBook}
    importingFolder={$folders.importingFolder}
    onimportfolder={folders.importFolder}
    onfolder={folders.goToFolder}
  />
  {#if $folders.error}<StatusState
      kind="error"
      message={$folders.error}
      actionLabel="Retry"
      onaction={folders.refresh}
    />{/if}
{/if}
{#if $folders.showCreateFolder}<CreateFolderDialog
    onclose={folders.closeCreateFolder}
    oncreate={folders.create}
  />{/if}
{#if $folders.pickerOperation}<FolderPickerDialog
    title={$folders.pickerOperation.kind === "copy_book"
      ? `Copy “${$folders.pickerOperation.book.title || "Untitled"}”`
      : $folders.pickerOperation.kind === "move_book"
        ? `Move “${$folders.pickerOperation.book.title || "Untitled"}”`
        : `Move “${$folders.pickerOperation.folder.name}”`}
    actionLabel={$folders.pickerOperation.kind === "copy_book" ? "Copy" : "Move"}
    destinations={$folders.pickerDestinations}
    initialId={$folders.pickerDestinations[0]?.id ?? ""}
    onclose={folders.closePicker}
    onconfirm={folders.confirmPicker}
  />{/if}
