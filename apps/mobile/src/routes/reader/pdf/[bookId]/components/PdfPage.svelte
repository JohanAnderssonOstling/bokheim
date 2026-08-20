<script lang="ts">
  import type { ReaderAnnotation } from "../../../../../lib/shared/types";
  import type { PdfPageState, SearchResult } from "../pdf-reader-types";

  let {
    page,
    index,
    totalPages,
    pageGap = 0,
    shellAction,
    searchResults,
    selectedSearchResult,
    annotations,
    activeAnnotationId,
    onOpenAnnotation,
  }: {
    page: PdfPageState;
    index: number;
    totalPages: number;
    pageGap?: number;
    shellAction: (
      node: HTMLElement,
      index: number,
    ) => void | {
      destroy?: () => void;
    };
    searchResults: Array<{ result: SearchResult; resultIndex: number }>;
    selectedSearchResult: number;
    annotations: ReaderAnnotation[];
    activeAnnotationId: string | null;
    onOpenAnnotation: (annotation: ReaderAnnotation) => void;
  } = $props();
</script>

<section
  use:shellAction={index}
  class="pdf-page"
  data-page-index={index}
  aria-label={`PDF page ${index + 1}`}
  style={`width:${page.displayWidth}px;height:${page.displayHeight}px;--page-gap:${index + 1 < totalPages ? pageGap : 0}px`}
>
  <canvas></canvas>
  <div
    class="pdf-search-highlight-layer"
    aria-hidden="true"
  >
    {#each searchResults as { result, resultIndex } (`${resultIndex}-${result.startItem}-${result.startOffset}`)}
      {#each result.rects as rect, rectIndex (`${resultIndex}-${rectIndex}`)}
        <span
          class:active={resultIndex === selectedSearchResult}
          style={`left:${rect.left * 100}%;top:${rect.top * 100}%;width:${rect.width * 100}%;height:${rect.height * 100}%`}
        ></span>
      {/each}
    {/each}
  </div>
  <div
    class="pdf-annotation-layer"
    aria-label={`Annotations on page ${index + 1}`}
  >
    {#each annotations as annotation}
      {@const anchor = annotation.anchor.kind === "pdf" ? annotation.anchor : null}
      {#each anchor?.rects ?? [] as rect, rectIndex (`${annotation.id}-${rectIndex}`)}
        <button
          class:active={activeAnnotationId === annotation.id}
          class:highlight={annotation.style === "highlight"}
          class:underline={annotation.style === "underline"}
          class:squiggly={annotation.style === "squiggly"}
          class:strikethrough={annotation.style === "strikethrough"}
          type="button"
          aria-label={`Open annotation: ${annotation.exact_text}`}
          style={`left:${rect.left * 100}%;top:${rect.top * 100}%;width:${rect.width * 100}%;height:${rect.height * 100}%;--annotation-color:${annotation.color}`}
          onclick={(event) => {
            event.stopPropagation();
            onOpenAnnotation(annotation);
          }}
        ></button>
      {/each}
    {/each}
  </div>
  <div class="pdf-text-layer textLayer"></div>
  {#if page.rendering}
    <span
      class="page-loading"
      aria-label={`Rendering page ${index + 1}`}
    ></span>
  {/if}
</section>

<style>
  .pdf-page {
    position: relative;
    flex: none;
    margin: 0 0 var(--page-gap);
    overflow: hidden;
    background: white;
    box-shadow: 0 1px 2px color-mix(in srgb, black 28%, transparent);
  }

  .pdf-page canvas,
  .pdf-text-layer,
  .pdf-search-highlight-layer,
  .pdf-annotation-layer {
    position: absolute;
    inset: 0;
    display: block;
  }
  .pdf-page canvas {
    z-index: 0;
  }
  .pdf-search-highlight-layer {
    z-index: 1;
    pointer-events: none;
  }
  .pdf-search-highlight-layer span {
    position: absolute;
    background: rgb(255 210 0 / 38%);
    outline: 1px solid rgb(180 120 0 / 45%);
  }
  .pdf-search-highlight-layer span.active {
    background: rgb(255 145 0 / 58%);
    outline-color: rgb(150 75 0 / 75%);
  }
  .pdf-annotation-layer {
    z-index: 3;
    pointer-events: none;
  }
  .pdf-annotation-layer button {
    position: absolute;
    min-width: 0;
    height: auto;
    padding: 0;
    border: 0;
    background: transparent;
    pointer-events: auto;
  }
  .pdf-annotation-layer button.highlight {
    background: color-mix(in srgb, var(--annotation-color) 38%, transparent);
  }
  .pdf-annotation-layer button.underline {
    border-bottom: 2px solid var(--annotation-color);
  }
  .pdf-annotation-layer button.squiggly {
    border-bottom: 2px dotted var(--annotation-color);
  }
  .pdf-annotation-layer button.strikethrough {
    border-top: 2px solid var(--annotation-color);
    transform: translateY(52%);
  }
  .pdf-annotation-layer button.active {
    outline: 1px solid var(--annotation-color);
    outline-offset: 1px;
  }

  .pdf-text-layer {
    --scale-factor: 1;
    --user-unit: 1;
    --total-scale-factor: calc(var(--scale-factor) * var(--user-unit));
    --scale-round-x: 1px;
    --scale-round-y: 1px;
    overflow: hidden;
    line-height: 1;
    opacity: 1;
    z-index: 2;
  }

  .page-loading {
    position: absolute;
    inset: 50% auto auto 50%;
    width: 22px;
    height: 22px;
    margin: -11px;
    border: 2px solid #777;
    border-radius: 50%;
  }
</style>
