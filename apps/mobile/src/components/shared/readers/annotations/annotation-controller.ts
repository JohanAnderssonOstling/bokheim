import type { ReaderAnnotation, ReaderAnnotationStyle } from "$lib/shared/types.ts";

type ErrorMessage = string;

export interface AnnotationControllerCallbacks {
  canUpsert?: () => boolean;
  canDelete?: () => boolean;
  getActiveAnnotation: () => ReaderAnnotation | null;
  setActiveAnnotation: (annotation: ReaderAnnotation | null) => void;
  getAnnotations: () => ReaderAnnotation[];
  setAnnotations: (annotations: ReaderAnnotation[]) => void;
  upsertAnnotation: (annotation: ReaderAnnotation) => Promise<void> | void;
  deleteAnnotation: (annotation: ReaderAnnotation) => Promise<void> | void;
  onPersistError: (error: ErrorMessage) => void;
  onDeleteError?: (error: ErrorMessage) => void;
  onAnnotationChange?: (previous: ReaderAnnotation, next: ReaderAnnotation) => void;
  onBeforeClose?: (annotation: ReaderAnnotation) => void;
  onBeforeDelete?: (annotation: ReaderAnnotation) => void;
}

export interface AnnotationController {
  beginEditing(annotation: ReaderAnnotation): void;
  updateDraft(note: string): void;
  closeActiveAnnotation(): void;
  setActiveAnnotationStyle(nextStyle: ReaderAnnotationStyle): void;
  setActiveAnnotationColor(nextColor: string): void;
  deleteAnnotation(annotation: ReaderAnnotation): void;
  deleteActiveAnnotation(): void;
  clearPending(): void;
  dispose(): void;
}

function toErrorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function replaceAnnotationInList(
  annotations: ReaderAnnotation[],
  next: ReaderAnnotation,
) {
  return annotations.map((item) => (item.id === next.id ? next : item));
}

export function createAnnotationController(
  callbacks: AnnotationControllerCallbacks,
  saveDelayMs = 450,
): AnnotationController {
  let annotationSaveQueue = Promise.resolve();
  let annotationSaveTimer: ReturnType<typeof setTimeout> | undefined;

  let annotationSavedDraft = "";
  let annotationSavedStyle: ReaderAnnotationStyle = "highlight";
  let annotationSavedColor = "#f6c945";
  let annotationDraft = "";

  function queueAnnotationSave(annotation: ReaderAnnotation, note: string) {
    if (callbacks.canUpsert && !callbacks.canUpsert()) return;
    if (
      note === annotationSavedDraft &&
      annotation.style === annotationSavedStyle &&
      annotation.color === annotationSavedColor
    )
      return;

    annotationSavedDraft = note;
    annotationSavedStyle = annotation.style;
    annotationSavedColor = annotation.color;

    const updated: ReaderAnnotation = { ...annotation, note, modified_at: Date.now() };
    const active = callbacks.getActiveAnnotation();
    callbacks.setAnnotations(
      replaceAnnotationInList(callbacks.getAnnotations(), updated),
    );
    if (active?.id === updated.id) callbacks.setActiveAnnotation(updated);

    annotationSaveQueue = annotationSaveQueue
      .then(async () => {
        await callbacks.upsertAnnotation(updated);
      })
      .catch((reason) => {
        callbacks.onPersistError(toErrorMessage(reason));
      });
  }

  function scheduleAnnotationSave(note: string) {
    const annotation = callbacks.getActiveAnnotation();
    if (!annotation) return;
    if (annotationSaveTimer) clearTimeout(annotationSaveTimer);
    annotationSaveTimer = setTimeout(() => {
      annotationSaveTimer = undefined;
      queueAnnotationSave(annotation, note);
    }, saveDelayMs);
  }

  function updateDraft(note: string) {
    annotationDraft = note;
    scheduleAnnotationSave(note);
  }

  function closeActiveAnnotation() {
    const annotation = callbacks.getActiveAnnotation();
    if (!annotation) return;
    if (annotationSaveTimer) {
      clearTimeout(annotationSaveTimer);
      annotationSaveTimer = undefined;
    }
    queueAnnotationSave(annotation, annotationDraft);
    callbacks.onBeforeClose?.(annotation);
    callbacks.setActiveAnnotation(null);
  }

  function updateAnnotationStyle(
    nextStyle: ReaderAnnotationStyle,
    note = annotationDraft,
  ) {
    const annotation = callbacks.getActiveAnnotation();
    if (!annotation || annotation.style === nextStyle) return;
    const previous = annotation;
    const updated: ReaderAnnotation = {
      ...annotation,
      style: nextStyle,
      modified_at: Date.now(),
    };
    callbacks.onAnnotationChange?.(previous, updated);
    if (annotationSaveTimer) {
      clearTimeout(annotationSaveTimer);
      annotationSaveTimer = undefined;
    }
    callbacks.setAnnotations(
      replaceAnnotationInList(callbacks.getAnnotations(), updated),
    );
    callbacks.setActiveAnnotation(updated);
    queueAnnotationSave(updated, note);
  }

  function updateAnnotationColor(nextColor: string, note = annotationDraft) {
    const annotation = callbacks.getActiveAnnotation();
    if (!annotation || annotation.color === nextColor) return;
    const previous = annotation;
    const updated: ReaderAnnotation = {
      ...annotation,
      color: nextColor,
      modified_at: Date.now(),
    };
    callbacks.onAnnotationChange?.(previous, updated);
    if (annotationSaveTimer) {
      clearTimeout(annotationSaveTimer);
      annotationSaveTimer = undefined;
    }
    callbacks.setAnnotations(
      replaceAnnotationInList(callbacks.getAnnotations(), updated),
    );
    callbacks.setActiveAnnotation(updated);
    queueAnnotationSave(updated, note);
  }

  function deleteActiveAnnotation() {
    const annotation = callbacks.getActiveAnnotation();
    if (!annotation) return;
    if (annotationSaveTimer) {
      clearTimeout(annotationSaveTimer);
      annotationSaveTimer = undefined;
    }

    annotationSavedDraft = "";
    callbacks.setActiveAnnotation(null);
    callbacks.onBeforeDelete?.(annotation);

    if (callbacks.canDelete && !callbacks.canDelete()) return;

    annotationSaveQueue = annotationSaveQueue
      .then(async () => {
        await callbacks.deleteAnnotation(annotation);
      })
      .catch((reason) => {
        callbacks.onDeleteError?.(toErrorMessage(reason));
      });
  }

  function deleteAnnotation(annotation: ReaderAnnotation) {
    if (callbacks.getActiveAnnotation()?.id === annotation.id) {
      deleteActiveAnnotation();
      return;
    }
    if (callbacks.canDelete && !callbacks.canDelete()) return;
    callbacks.onBeforeDelete?.(annotation);
    annotationSaveQueue = annotationSaveQueue
      .then(async () => {
        await callbacks.deleteAnnotation(annotation);
      })
      .catch((reason) => {
        callbacks.onDeleteError?.(toErrorMessage(reason));
      });
  }

  function clearPending() {
    if (annotationSaveTimer) {
      clearTimeout(annotationSaveTimer);
      annotationSaveTimer = undefined;
    }
  }

  function dispose() {
    clearPending();
  }

  function beginEditing(annotation: ReaderAnnotation) {
    annotationDraft = annotation.note;
    annotationSavedDraft = annotation.note;
    annotationSavedStyle = annotation.style;
    annotationSavedColor = annotation.color;
  }

  return {
    beginEditing,
    updateDraft,
    closeActiveAnnotation,
    setActiveAnnotationStyle: updateAnnotationStyle,
    setActiveAnnotationColor: updateAnnotationColor,
    deleteAnnotation,
    deleteActiveAnnotation,
    clearPending,
    dispose,
  };
}
