<script lang="ts">
  import { goto } from "$app/navigation";
  import { page } from "$app/state";
  import AuthorsPage from "./AuthorsPage.svelte";
  import StatusState from "../../../../components/shared/StatusState.svelte";
  import { createAuthorsRoute } from "./authors-route";
  import {
    onBookDownloadStatus,
    onLibraryRefresh,
  } from "../../../../lib/app/library-events";

  const authors = createAuthorsRoute((path) => goto(path));
  $effect(() => void authors.load(page.params.libraryId ?? ""));
  $effect(() =>
    onLibraryRefresh(() => void authors.load(page.params.libraryId ?? "", true)),
  );
  $effect(() => {
    const authorId = page.url.searchParams.get("author");
    const authorName = page.url.searchParams.get("name");
    if (authorId && authorName) void authors.select(authorId, authorName);
  });
  $effect(() =>
    onBookDownloadStatus((change) =>
      authors.setDownloadState(change.contentHash, change.state),
    ),
  );
</script>

<svelte:head>
  <title>Authors · Bokheim</title>
</svelte:head>

{#if $authors.loading}
  <StatusState message="Loading authors" />
{:else}
  <AuthorsPage
    libraryId={$authors.libraryId}
    authors={$authors.authors}
    filter={$authors.filter}
    focusedIndex={$authors.focusedIndex}
    selectedAuthorId={$authors.selectedAuthorId}
    selectedAuthorName={$authors.selectedAuthorName}
    books={$authors.books}
    detailed={$authors.detailed}
    openingBookId=""
    downloadStates={$authors.downloadStates}
    onfilterkeydown={authors.filterKeydown}
    onauthorkeydown={authors.authorKeydown}
    onselectauthor={authors.select}
    onclear={authors.clear}
    onadd={authors.add}
    onopen={authors.open}
    onopenbook={authors.openBook}
    ondownload={authors.download}
    onremove={authors.remove}
    ontrash={authors.trash}
  />
{/if}
