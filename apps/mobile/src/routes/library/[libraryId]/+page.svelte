<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import HomePage from "./HomePage.svelte";
  import StatusState from "../../../components/shared/StatusState.svelte";
  import { createHomePageController } from "./home-page";
  import {
    onBookDownloadStatus,
    onLibraryRefresh,
  } from "../../../lib/app/library-events";

  const home = createHomePageController((path) => goto(path));

  $effect(() => {
    void home.load(page.params.libraryId ?? "");
  });

  $effect(() =>
    onLibraryRefresh(() => void home.load(page.params.libraryId ?? "", true)),
  );
  $effect(() =>
    onBookDownloadStatus((change) =>
      home.setDownloadState(change.contentHash, change.state),
    ),
  );
</script>

<svelte:head>
  <title>Library · Bokheim</title>
</svelte:head>

{#if $home.loading}
  <StatusState
    kind="loading"
    message="Loading library"
  />
{:else}
  <HomePage
    libraryId={$home.libraryId}
    continueReading={$home.continueReading}
    inProgress={$home.inProgress}
    finished={$home.finished}
    recentlyAdded={$home.recentlyAdded}
    openingBookId=""
    downloadStates={$home.downloadStates}
    onadd={home.add}
    onopen={home.open}
    ondownload={home.download}
    onremove={home.remove}
    ontrash={home.trash}
  />
  {#if $home.error}
    <StatusState
      kind="error"
      message={$home.error}
      actionLabel="Retry"
      onaction={() => home.load($home.libraryId, true)}
    />
  {/if}
{/if}
