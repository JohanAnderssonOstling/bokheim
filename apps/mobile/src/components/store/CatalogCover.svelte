<script lang="ts">
  import { getCatalogCoverUrl } from "../../lib/catalog-cover-cache";

  let { sourceId, imageUrl, title }: { sourceId: string; imageUrl: string | null; title: string } = $props();
  let element: HTMLElement;
  let coverUrl = $state("");

  $effect(() => {
    const currentSourceId = sourceId;
    const currentImageUrl = imageUrl;
    let active = true;
    coverUrl = "";
    if (!currentImageUrl) return;
    const load = () => void getCatalogCoverUrl(currentSourceId, currentImageUrl).then((url) => { if (active && url) coverUrl = url; });
    if (!("IntersectionObserver" in window)) {
      load();
      return () => { active = false; };
    }
    const observer = new IntersectionObserver((entries) => {
      if (!entries.some((entry) => entry.isIntersecting)) return;
      observer.disconnect();
      load();
    }, { rootMargin: "160px" });
    observer.observe(element);
    return () => {
      active = false;
      observer.disconnect();
    };
  });
</script>

<span class="catalog-cover" bind:this={element} aria-hidden="true">
  {#if coverUrl}<img src={coverUrl} alt="" decoding="async" />{:else}<span>{title.slice(0, 1)}</span>{/if}
</span>

<style>
  .catalog-cover { display: grid; width: 100%; height: 100%; place-items: center; }
  .catalog-cover img { width: 100%; height: 100%; object-fit: contain; object-position: center top; }
</style>
