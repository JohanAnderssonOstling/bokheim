<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import FolderPickerDialog from "../../../../../components/shared/dialogs/FolderPickerDialog.svelte";
  import StatusState from "../../../../../components/shared/StatusState.svelte";
  import BookDetailPage from "./BookDetailPage.svelte";
  import { createBookDetailRoute } from "./book-detail-route";
  import {
    onBookDownloadStatus,
    onLibraryRefresh,
  } from "../../../../../lib/app/library-events";

  const detail = createBookDetailRoute(
    (path) => goto(path),
    () => history.back(),
  );
  $effect(() => void detail.load(page.params.libraryId ?? "", page.params.bookId ?? ""));
  $effect(() =>
    onLibraryRefresh(
      () => void detail.load(page.params.libraryId ?? "", page.params.bookId ?? ""),
    ),
  );
  $effect(() =>
    onBookDownloadStatus((change) =>
      detail.setDownloadState(change.contentHash, change.state),
    ),
  );
</script>

<svelte:head><title>Book · Bokheim</title></svelte:head>
{#if $detail.loading}
  <StatusState
    kind="loading"
    variant="compact"
    title="Opening book details"
    actionLabel="Back"
    onaction={detail.back}
  />
{:else if $detail.error}
  <StatusState
    kind="error"
    variant="compact"
    title="Book unavailable"
    message={$detail.error}
    actionLabel="Back"
    onaction={detail.back}
  />
{:else if $detail.detail}
  <BookDetailPage
    detail={$detail.detail}
    libraryId={$detail.libraryId}
    openingBookId=""
    downloadStates={$detail.downloadStates}
    onback={detail.back}
    onread={detail.read}
    onrelated={detail.related}
    onauthor={detail.author}
    onsubject={detail.subject}
    ondownload={detail.download}
    onremove={detail.remove}
    ontrash={detail.trash}
    oncopy={detail.copy}
    onmove={detail.move}
  />
{/if}
{#if $detail.pickerMode}<FolderPickerDialog
    title={$detail.pickerMode === "copy" ? "Add to another folder" : "Move to folder"}
    actionLabel={$detail.pickerMode === "copy" ? "Copy" : "Move"}
    destinations={$detail.pickerDestinations}
    initialId={$detail.pickerDestinations[0]?.id ?? ""}
    onclose={detail.closePicker}
    onconfirm={detail.confirmPicker}
  />{/if}
