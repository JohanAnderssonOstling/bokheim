<script lang="ts">
  import { getCatalogCoverUrl } from "./cover-cache";
  import BookCover from "../../../components/shared/BookCover.svelte";

  let {
    sourceId,
    imageUrl,
    title,
    disabled = false,
    ariaLabel,
    onclick,
  }: {
    sourceId: string;
    imageUrl: string | null;
    title: string;
    disabled?: boolean;
    ariaLabel?: string;
    onclick?: (event: MouseEvent) => void | Promise<void>;
  } = $props();
  let element: HTMLElement;
  let coverUrl = $state("");

  $effect(() => {
    const currentSourceId = sourceId;
    const currentImageUrl = imageUrl;
    let active = true;
    coverUrl = "";
    if (!currentImageUrl) return;
    const load = () =>
      void getCatalogCoverUrl(currentSourceId, currentImageUrl).then((url) => {
        if (active && url) coverUrl = url;
      });
    if (!("IntersectionObserver" in window)) {
      load();
      return () => {
        active = false;
      };
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer.disconnect();
        load();
      },
      { rootMargin: "160px" },
    );
    observer.observe(element);
    return () => {
      active = false;
      observer.disconnect();
    };
  });
</script>

<span
  class="catalog-cover-anchor"
  bind:this={element}
>
  <BookCover
    src={coverUrl}
    {title}
    fit="contain"
    className="catalog-cover"
    {disabled}
    {ariaLabel}
    {onclick}
  />
</span>

<style>
  .catalog-cover-anchor {
    display: grid;
    width: 100%;
    height: 100%;
  }
  :global(.catalog-cover) {
    display: grid;
    width: 100%;
    height: 100%;
    place-items: center;
  }
</style>
