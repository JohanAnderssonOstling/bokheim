import { tick } from "svelte";
import type {
  ReaderAnnotation,
  ReaderAnnotationStyle,
} from "../../../../lib/shared/types";
import { createAnnotationController } from "../../../../components/shared/readers/annotations/annotation-controller.ts";
import { centerAnchor, placeAnnotationPopup } from "./annotation-placement";
import type {
  PopupPlacement,
  ViewportRect,
} from "../../../../components/shared/readers/annotations/annotation-placement";

/// The note field grows with its content up to this height, then scrolls.
const NOTE_MAX_HEIGHT_PX = 160;

export interface ReaderAnnotationEditorOptions {
  canUpsert: () => boolean;
  canDelete: () => boolean;
  getAnnotations: () => ReaderAnnotation[];
  setAnnotations: (annotations: ReaderAnnotation[]) => void;
  upsertAnnotation: (annotation: ReaderAnnotation) => Promise<void> | void;
  deleteAnnotation: (annotation: ReaderAnnotation) => Promise<void> | void;
  /// Redraws the overlay after a style or colour change.
  replaceOverlay: (previous: ReaderAnnotation, next: ReaderAnnotation) => void;
  /// Where the selection sits on screen; the popup opens beside it.
  getAnchorRect: () => ViewportRect | null;
  clearAnchorRect: () => void;
  closeContextMenu: () => void;
  readerState: { error: string };
}

/**
 * Drives the popup that edits one annotation's note, style and colour.
 *
 * The popup is measured after it renders, so opening it is a two-step process:
 * mount with no position, then place it once its size is known. Every change is
 * autosaved by the shared annotation behaviour, so there is no explicit save.
 */
export function createReaderAnnotationEditor(options: ReaderAnnotationEditorOptions) {
  let active = $state<ReaderAnnotation | null>(null);
  let draft = $state("");
  let position = $state<{ x: number; y: number; placement: PopupPlacement } | null>(null);
  let noteInput = $state<HTMLTextAreaElement | null>(null);
  let popupElement = $state<HTMLElement | undefined>();

  const controller = createAnnotationController({
    canUpsert: options.canUpsert,
    canDelete: options.canDelete,
    getActiveAnnotation: () => active,
    setActiveAnnotation: (annotation) => {
      active = annotation;
    },
    getAnnotations: options.getAnnotations,
    setAnnotations: options.setAnnotations,
    upsertAnnotation: options.upsertAnnotation,
    deleteAnnotation: options.deleteAnnotation,
    onPersistError: (message) => (options.readerState.error = message),
    onDeleteError: (message) => (options.readerState.error = message),
    onAnnotationChange: options.replaceOverlay,
    onBeforeClose: () => {
      position = null;
      options.clearAnchorRect();
    },
    onBeforeDelete: (annotation) => {
      if (active?.id !== annotation.id) return;
      position = null;
      options.clearAnchorRect();
      options.closeContextMenu();
    },
  });

  function resizeNoteInput() {
    if (!noteInput) return;
    noteInput.style.height = "auto";
    noteInput.style.height = `${Math.min(noteInput.scrollHeight, NOTE_MAX_HEIGHT_PX)}px`;
  }

  /**
   * Places the popup beside its anchor, or centred on the publication when the
   * annotation was opened from the sidebar and has no on-screen range.
   */
  function reposition() {
    if (!popupElement || !active) return;
    const viewport = document
      .querySelector<HTMLElement>(".viewport")
      ?.getBoundingClientRect();
    const anchor =
      options.getAnchorRect() ??
      centerAnchor(
        viewport ?? {
          left: 0,
          top: 0,
          right: window.innerWidth,
          bottom: window.innerHeight,
        },
      );
    position = placeAnnotationPopup(anchor, popupElement.getBoundingClientRect(), {
      width: window.innerWidth,
      height: window.innerHeight,
    });
  }

  async function open(annotation: ReaderAnnotation) {
    if (active && active.id !== annotation.id) controller.closeActiveAnnotation();
    active = annotation;
    draft = annotation.note;
    controller.beginEditing(annotation);
    position = null;
    await tick();
    resizeNoteInput();
    reposition();
    noteInput?.focus();
  }

  function updateDraft(note: string) {
    draft = note;
    resizeNoteInput();
    // The popup may need to move once the grown note has been laid out.
    requestAnimationFrame(reposition);
    controller.updateDraft(note);
  }

  /// True when the event happened outside the popup, so it should close.
  function isOutside(target: Element | null) {
    return !target?.closest?.(".annotation-popup, .reader-annotation-editor");
  }

  return {
    get active() {
      return active;
    },
    get draft() {
      return draft;
    },
    get position() {
      return position;
    },
    get placement() {
      return position?.placement;
    },
    setNoteInput: (node: HTMLTextAreaElement | null) => (noteInput = node),
    setPopupElement: (node: HTMLElement | undefined) => (popupElement = node),
    open,
    updateDraft,
    reposition,
    isOutside,
    close: () => controller.closeActiveAnnotation(),
    setStyle: (style: ReaderAnnotationStyle) =>
      controller.setActiveAnnotationStyle(style),
    setColor: (color: string) => controller.setActiveAnnotationColor(color),
    deleteActive: () => controller.deleteActiveAnnotation(),
    deleteAnnotation: (annotation: ReaderAnnotation) =>
      controller.deleteAnnotation(annotation),
    dispose: controller.dispose,
  };
}
