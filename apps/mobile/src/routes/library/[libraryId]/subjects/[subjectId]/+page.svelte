<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import BookGrid from "../../../../../components/shared/books/BookGrid.svelte";
  import StatusState from "../../../../../components/shared/StatusState.svelte";
  import SubjectsPage from "./SubjectsPage.svelte";
  import { createSubjectsRoute } from "./subjects-route";
  import {
    onBookDownloadStatus,
    onLibraryRefresh,
  } from "../../../../../lib/app/library-events";

  const subjects = createSubjectsRoute((path) => goto(path));

  $effect(() => {
    void subjects.load(page.params.libraryId ?? "", page.params.subjectId ?? "__root__");
  });

  $effect(() =>
    onLibraryRefresh(
      () =>
        void subjects.load(
          page.params.libraryId ?? "",
          page.params.subjectId ?? "__root__",
          true,
        ),
    ),
  );
  $effect(() =>
    onBookDownloadStatus((change) =>
      subjects.setDownloadState(change.contentHash, change.state),
    ),
  );
</script>

<svelte:head>
  <title>Subjects · Bokheim</title>
</svelte:head>

{#snippet searchResults()}
  <div
    class="scoped-search-results"
    aria-live="polite"
  >
    {#if $subjects.searchPending}
      <div class="inline-empty">Searching…</div>
    {:else if $subjects.searchCompletedQuery}
      {#if $subjects.titleSearchResults.length}
        <h2>Title matches</h2>
        <BookGrid
          libraryId={$subjects.libraryId}
          books={$subjects.titleSearchResults}
          detailed={$subjects.detailed}
          emptyMessage=""
          openingBookId=""
          downloadStates={$subjects.downloadStates}
          onopen={subjects.open}
          ondownload={subjects.download}
          onremove={subjects.remove}
          ontrash={subjects.trash}
        />
      {/if}
      {#if $subjects.authorSearchResults.length}
        <h2>Author matches</h2>
        <BookGrid
          libraryId={$subjects.libraryId}
          books={$subjects.authorSearchResults}
          detailed={$subjects.detailed}
          emptyMessage=""
          openingBookId=""
          downloadStates={$subjects.downloadStates}
          onopen={subjects.open}
          ondownload={subjects.download}
          onremove={subjects.remove}
          ontrash={subjects.trash}
        />
      {/if}
      {#if !$subjects.titleSearchResults.length && !$subjects.authorSearchResults.length}
        <div class="inline-empty">No matching books</div>
      {/if}
    {/if}
  </div>
{/snippet}

{#if $subjects.loading}
  <StatusState message="Loading subjects" />
{:else}
  <SubjectsPage
    libraryId={$subjects.libraryId}
    subjects={$subjects.content.children}
    books={$subjects.content.books}
    subjectPath={$subjects.subjectPath}
    fileType={$subjects.fileType}
    loading={$subjects.loading}
    query={$subjects.query}
    detailed={$subjects.detailed}
    openingBookId=""
    downloadStates={$subjects.downloadStates}
    {searchResults}
    onsearch={subjects.scheduleSearch}
    onquerychange={subjects.setQuery}
    onfiletype={subjects.setFileType}
    onopensubject={(subject) => subjects.openSubject(subject.id)}
    onimported={subjects.refresh}
    onopen={subjects.open}
    ondownload={subjects.download}
    onremove={subjects.remove}
    ontrash={subjects.trash}
    onsubject={subjects.goToSubject}
  />
  {#if $subjects.error}
    <StatusState
      kind="error"
      message={$subjects.error}
      actionLabel="Retry"
      onaction={subjects.refresh}
    />
  {/if}
{/if}
