<script lang="ts">
  import { catalogLinkCount } from "./links";
  import type { CatalogLink } from "../../../lib/bindings";
  import FolderNavigationCard from "../../../components/shared/browse/FolderNavigationCard.svelte";

  let {
    links,
    getLinkKey,
    loading = false,
    onselect,
  }: {
    links: CatalogLink[];
    getLinkKey: (link: CatalogLink) => string;
    loading?: boolean;
    onselect: (link: CatalogLink) => void | Promise<void>;
  } = $props();
</script>

{#if links.length}
  <div class="catalog-navigation-grid">
    {#each links as link (getLinkKey(link))}
      <div class="catalog-navigation-item">
        <FolderNavigationCard
          name={link.title}
          count={catalogLinkCount(link)}
          active={link.active}
          disabled={loading}
          onclick={() => void onselect(link)}
        />
      </div>
    {/each}
  </div>
{/if}

<style>
  .catalog-navigation-grid {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: 0.45rem;
    margin: 1rem 0 1.4rem;
  }

  .catalog-navigation-item {
    width: max-content;
    max-width: 100%;
    min-width: 0;
  }
</style>
