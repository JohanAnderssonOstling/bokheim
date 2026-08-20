<script lang="ts">
  import DangerButton from "../../buttons/DangerButton.svelte";
  import PrimaryButton from "../../buttons/PrimaryButton.svelte";
  import SecondaryButton from "../../buttons/SecondaryButton.svelte";
  import type { ReaderAnnotation, ReaderAnnotationStyle } from "$lib/shared/types.ts";

  const annotationStyleColors = [
    { label: "Yellow", value: "#f6c945" },
    { label: "Blue", value: "#60a5fa" },
    { label: "Pink", value: "#f472b6" },
    { label: "Green", value: "#4ade80" },
  ] as const;

  let {
    annotation,
    note,
    style = "",
    position,
    placement,
    onNoteChange,
    onDelete,
    onClose,
    onSave,
    onStyleChange,
    onColorChange,
    onTextareaMount,
    rootClass = "",
    ariaLabel = "Annotation",
    dataPlacement,
    placeholder = "Add a note",
    noteAriaLabel = "Annotation note",
    closeLabel = "Close",
    saveLabel = "Save",
    deleteLabel = "Delete",
    showDelete = false,
    showClose = false,
    showSave = false,
    actionsClass = "reader-annotation-editor-actions",
    textareaClass = "",
  }: {
    annotation: ReaderAnnotation;
    note: string;
    style?: string;
    position?: { x: number; y: number } | null;
    placement?: string;
    onNoteChange: (note: string) => void;
    onDelete?: () => void;
    onClose?: () => void;
    onSave?: () => void;
    onStyleChange: (style: ReaderAnnotationStyle) => void;
    onColorChange: (color: string) => void;
    onTextareaMount?: (node: HTMLTextAreaElement | null) => void;
    rootClass?: string;
    ariaLabel?: string;
    dataPlacement?: string;
    placeholder?: string;
    noteAriaLabel?: string;
    closeLabel?: string;
    saveLabel?: string;
    deleteLabel?: string;
    showDelete?: boolean;
    showClose?: boolean;
    showSave?: boolean;
    actionsClass?: string;
    textareaClass?: string;
  } = $props();

  let textareaElement: HTMLTextAreaElement | null = null;

  $effect(() => {
    onTextareaMount?.(textareaElement);
  });

  const popupStyle = $derived(
    position !== undefined
      ? position
        ? `left: ${position.x}px; top: ${position.y}px;`
        : "left: 0; top: 0; visibility: hidden;"
      : style || "",
  );
  const rootClassName = $derived(
    rootClass && rootClass.trim()
      ? `reader-annotation-editor ${rootClass}`
      : "reader-annotation-editor",
  );
  function handleNoteChange(event: Event) {
    onNoteChange((event.currentTarget as HTMLTextAreaElement).value);
  }
</script>

<section
  class={`reader-annotation-popup ${rootClassName}`.trim()}
  aria-label={ariaLabel}
  data-placement={dataPlacement ?? placement}
  style={popupStyle}
>
  <q>{annotation.exact_text}</q>
  <textarea
    bind:this={textareaElement}
    value={note}
    class={textareaClass}
    {placeholder}
    aria-label={noteAriaLabel}
    oninput={handleNoteChange}></textarea>
  <div class="reader-annotation-editor-toolbar">
    <div
      class="annotation-options"
      role="group"
      aria-label="Annotation style"
    >
      <button
        class:active={annotation.style === "highlight"}
        type="button"
        onclick={() => onStyleChange("highlight")}
      >
        Highlight
      </button>
      <button
        class:active={annotation.style === "underline"}
        type="button"
        onclick={() => onStyleChange("underline")}
      >
        Underline
      </button>
      <button
        class:active={annotation.style === "squiggly"}
        type="button"
        onclick={() => onStyleChange("squiggly")}
      >
        Squiggly
      </button>
      <button
        class:active={annotation.style === "strikethrough"}
        type="button"
        onclick={() => onStyleChange("strikethrough")}
      >
        Strike
      </button>
    </div>
    <div
      class="annotation-colors"
      role="group"
      aria-label="Annotation color"
    >
      {#each annotationStyleColors as color}
        <button
          class:active={annotation.color === color.value}
          type="button"
          style={`--swatch:${color.value}`}
          onclick={() => onColorChange(color.value)}
        >
          {color.label}
        </button>
      {/each}
    </div>
  </div>
  {#if showDelete || showClose || showSave}
    <div class={actionsClass || "reader-annotation-popup-actions"}>
      {#if showDelete}
        <DangerButton onclick={onDelete}>{deleteLabel}</DangerButton>
      {/if}
      {#if showClose}
        <SecondaryButton onclick={onClose}>{closeLabel}</SecondaryButton>
      {/if}
      {#if showSave}
        <PrimaryButton onclick={onSave}>{saveLabel}</PrimaryButton>
      {/if}
    </div>
  {/if}
</section>

<style>
  :global(.reader-annotation-editor) {
    position: fixed;
    z-index: 24;
    max-height: min(520px, calc(100dvh - 24px));
    overflow: auto;
    box-sizing: border-box;
    padding: 10px;
    border: 1px solid var(--color-border);
    color: var(--color-ink-text);
    background: var(--color-surface-bg);
    box-shadow: 0 14px 36px color-mix(in srgb, black 28%, transparent);
  }

  :global(.reader-annotation-editor.annotation-editor) {
    z-index: 25;
  }

  :global(.reader-annotation-editor q) {
    display: block;
    margin-bottom: 9px;
    line-height: 1.35;
  }

  :global(.reader-annotation-editor textarea) {
    width: 100%;
    min-height: 72px;
    padding: 8px;
    box-sizing: border-box;
    resize: vertical;
    border: 1px solid var(--color-border);
    color: inherit;
    background: var(--color-raised-bg);
    font: inherit;
  }

  :global(.reader-annotation-editor .reader-annotation-editor-actions),
  :global(.reader-annotation-editor .annotation-editor-actions),
  :global(.reader-annotation-editor .annotation-options),
  :global(.reader-annotation-editor .annotation-colors) {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    margin-top: 8px;
  }

  :global(.reader-annotation-editor .reader-annotation-editor-actions),
  :global(.reader-annotation-editor .annotation-editor-actions) {
    justify-content: flex-end;
    padding-top: 8px;
    border-top: 1px solid var(--color-border);
  }

  :global(.reader-annotation-editor .annotation-options button) {
    flex: 1;
    min-width: 72px;
  }

  :global(.reader-annotation-editor .annotation-colors button) {
    border-left: 8px solid var(--swatch);
  }

  :global(.reader-annotation-editor .reader-annotation-editor-toolbar) {
    margin-top: 8px;
    display: grid;
    gap: 5px;
  }

  /* Scoped to the two toggle groups rather than every button in the editor:
     Delete/Close/Save are shared button components, and a bare `button`
     selector here would outrank their own styling. */
  :global(.reader-annotation-editor .annotation-options button),
  :global(.reader-annotation-editor .annotation-colors button) {
    min-width: 34px;
    height: 34px;
    padding: 0 9px;
    border: 1px solid var(--color-border);
    border-radius: 0;
    color: inherit;
    background: var(--color-raised-bg);
    font: inherit;
    font-size: 0.82rem;
    font-weight: 700;
    white-space: nowrap;
  }

  :global(.reader-annotation-editor .annotation-options button:active),
  :global(.reader-annotation-editor .annotation-options button.active),
  :global(.reader-annotation-editor .annotation-colors button:active),
  :global(.reader-annotation-editor .annotation-colors button.active) {
    background: var(--color-border);
  }

  :global(.reader-annotation-editor .annotation-options button:disabled),
  :global(.reader-annotation-editor .annotation-colors button:disabled) {
    opacity: 0.38;
  }
</style>
