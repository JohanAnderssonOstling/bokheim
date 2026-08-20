<script lang="ts">
  import type { Snippet } from "svelte";
  import type { Destination } from "../../lib/app/navigation";

  let {
    active,
    detailMode = false,
    playerOpen = false,
    content,
  }: {
    active: Destination;
    detailMode?: boolean;
    playerOpen?: boolean;
    content: Snippet;
  } = $props();
</script>

<div class="app-column">
  <main
    class:player-open={playerOpen}
    class:detail-view={detailMode}
    class:library-view={!detailMode && (active === "folders" || active === "subjects")}
    class:authors-view={!detailMode && active === "authors"}
    class:store-view={!detailMode && active === "store"}
    class="content"
  >
    {@render content()}
  </main>
</div>

<style>
  .app-column {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100dvh;
  }
  .content {
    --content-section-gap: 18px;
    --content-inset-inline-start: max(var(--page-gutter), env(safe-area-inset-left));
    --content-inset-inline-end: max(var(--page-gutter), env(safe-area-inset-right));
    flex: 1;
    min-height: 0;
    width: 100%;
    max-width: none;
    margin: 0 auto;
    padding: 1.1rem var(--page-gutter) calc(4.9rem + env(safe-area-inset-bottom));
    padding-right: var(--content-inset-inline-end);
    padding-left: var(--content-inset-inline-start);
    overflow-y: scroll;
    overscroll-behavior-y: contain;
    scrollbar-gutter: stable;
    scrollbar-color: var(--color-border) transparent;
    scrollbar-width: thin;
  }
  .content::-webkit-scrollbar {
    width: 0.55rem;
  }
  .content::-webkit-scrollbar-track {
    background: transparent;
  }
  .content::-webkit-scrollbar-thumb {
    border: 0.14rem solid transparent;
    border-radius: 0;
    background: var(--color-border);
    background-clip: padding-box;
  }
  .content.library-view {
    padding-top: 0;
  }
  .content.authors-view {
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .content.detail-view {
    padding-top: 1.1rem;
  }
  .content.player-open {
    padding-bottom: calc(9.8rem + env(safe-area-inset-bottom));
  }
  @media (min-width: 45rem) {
    .content {
      padding-top: var(--layout-content-padding);
      padding-right: max(var(--page-gutter), env(safe-area-inset-right));
      padding-bottom: 3rem;
      padding-left: max(var(--page-gutter), env(safe-area-inset-left));
    }
    .content.player-open {
      padding-bottom: 7.5rem;
    }
    .content.authors-view {
      padding-bottom: 1.5rem;
    }
  }
</style>
