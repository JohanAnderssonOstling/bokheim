<!--
  One page's shell: a canvas pdf.js paints into, the text layer it builds over
  that for selection, and the highlight rects for whatever annotations sit on
  this page. Rendering itself is imperative pdf.js work against these nodes,
  done by the reader that owns the document — this stays a dumb shell so that
  work has one place to live instead of being split across every page.
-->
<script>
  let {
    index,
    width,
    height,
    annotations,
    activeAnnotationId,
    onOpenAnnotation,
    canvas = $bindable(),
    textLayer = $bindable(),
  } = $props();
</script>

<section
  class="pdf-page"
  data-page-index={index}
  aria-label={`Page ${index + 1}`}
  style={`width:${width}px;height:${height}px`}
>
  <canvas bind:this={canvas}></canvas>
  <div
    class="pdf-annotation-layer"
    aria-label={`Annotations on page ${index + 1}`}
  >
    {#each annotations as annotation (annotation.id)}
      {#each annotation.anchor.rects ?? [] as rect, rectIndex (rectIndex)}
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
  <div
    class="pdf-text-layer textLayer"
    bind:this={textLayer}
  ></div>
</section>

<style>
  .pdf-page {
    position: relative;
    flex: none;
    margin: 0 auto 1rem;
    overflow: hidden;
    background: white;
    box-shadow: 0 1px 2px color-mix(in srgb, black 28%, transparent);
  }

  .pdf-page canvas,
  .pdf-text-layer,
  .pdf-annotation-layer {
    position: absolute;
    inset: 0;
    display: block;
  }

  .pdf-page canvas {
    z-index: 0;
  }

  .pdf-annotation-layer {
    z-index: 2;
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
    z-index: 1;
  }
</style>
