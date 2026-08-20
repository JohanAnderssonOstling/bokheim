<script lang="ts">
  import {
    catalogActionKey,
    primaryCatalogAction,
    type CatalogImportAction,
  } from "../catalog/actions";
  import CatalogPublicationCard from "../catalog/CatalogPublicationCard.svelte";
  import BookGridLayout from "../../../components/shared/books/BookGridLayout.svelte";
  import type { CatalogPublication, CatalogSource } from "../../../lib/bindings";
  import { isNativeApp } from "../../../lib/app/platform";

  let {
    source,
    items,
    isOwned,
    actionStates,
    actionErrors,
    onaction,
  }: {
    source: CatalogSource;
    items: CatalogPublication[];
    isOwned: (source: CatalogSource, publication: CatalogPublication) => boolean;
    actionStates: Record<string, "idle" | "adding" | "added">;
    actionErrors: Record<string, string>;
    onaction: (
      source: CatalogSource,
      publication: CatalogPublication,
      action: CatalogImportAction,
    ) => void | Promise<void>;
  } = $props();

  const visibleItems = $derived(
    items.filter((publication) => !isOwned(source, publication)),
  );
</script>

{#if visibleItems.length}
  <BookGridLayout variant="catalogue">
    {#each visibleItems as publication (`${publication.identity.edition_id}:${publication.title}`)}
      {@const primaryAction = primaryCatalogAction(publication.actions, isNativeApp)}
      {@const actionKey = primaryAction ? catalogActionKey(primaryAction) : ""}
      <CatalogPublicationCard
        sourceId={source.id}
        {publication}
        action={primaryAction}
        actionState={actionStates[actionKey] ?? "idle"}
        actionError={actionErrors[actionKey] ?? ""}
        onaction={(action) => void onaction(source, publication, action)}
      />
    {/each}
  </BookGridLayout>
{/if}
