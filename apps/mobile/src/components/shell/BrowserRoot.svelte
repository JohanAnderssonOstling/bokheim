<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Destination } from "../../lib/app/navigation";
  import Sidebar from "./navigation/Sidebar.svelte";

  let {
    active,
    hrefFor,
    libraryName,
    style,
    content,
    onlibraries,
  }: {
    active: Destination;
    hrefFor: (destination: Destination) => string;
    libraryName: string;
    style: string;
    content: Snippet;
    onlibraries: () => void;
  } = $props();
</script>

<div
  class="adaptive-shell"
  {style}
>
  <Sidebar
    {active}
    {hrefFor}
    {libraryName}
    {onlibraries}
  />
  {@render content()}
</div>

<style>
  .adaptive-shell {
    --page-gutter: 1rem;
    height: 100dvh;
    min-height: 100dvh;
    overflow: hidden;
    background: var(--color-surface-bg);
  }
  @media (min-width: 45rem) {
    .adaptive-shell {
      --page-gutter: var(--layout-content-padding);
      display: grid;
      grid-template-columns: max-content minmax(0, 1fr);
    }
  }
  @media (min-width: 88rem) {
    .adaptive-shell {
      --page-gutter: 2rem;
    }
  }
</style>
