<script lang="ts">
  import { page } from "$app/state";
  import PdfReader from "./PdfReader.svelte";
  import StatusState from "../../../../components/shared/StatusState.svelte";
  import { createReaderRoute } from "../../reader-route.svelte";
  import { closeReader } from "../../reader-navigation";

  const route = createReaderRoute();

  $effect(() => {
    const libraryId = page.url.searchParams.get("library") ?? "";
    const target = page.url.searchParams.get("target") ?? undefined;
    void route.open(libraryId, page.params.bookId ?? "", target);
    return route.close;
  });
</script>

<svelte:head>
  <title>{route.book ? `${route.book.title} · Reader` : "PDF reader · Bokheim"}</title>
</svelte:head>

{#if route.error}
  <StatusState
    kind="error"
    title="Could not open the book"
    message={route.error}
    actionLabel="Back"
    onaction={closeReader}
  />
{:else if route.book}
  <PdfReader
    book={route.book}
    onclose={closeReader}
  />
{:else}
  <StatusState
    kind="loading"
    variant="reader"
    title="Opening reader"
  />
{/if}
