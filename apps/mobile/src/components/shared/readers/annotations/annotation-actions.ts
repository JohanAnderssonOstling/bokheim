import { getContext, setContext } from "svelte";
import type { ReaderAnnotation } from "$lib/shared/types.ts";

export interface ReaderAnnotationActions {
  readonly items: ReaderAnnotation[];
  activate(annotation: ReaderAnnotation): void | Promise<void>;
  delete(annotation: ReaderAnnotation): void | Promise<void>;
}

const READER_ANNOTATION_ACTIONS = Symbol("reader-annotation-actions");

/** Registers annotation mutations for descendants of one reader session. */
export function provideReaderAnnotationActions(actions: ReaderAnnotationActions) {
  setContext(READER_ANNOTATION_ACTIONS, actions);
}

/** Returns annotation mutations when the current reader provides them. */
export function useReaderAnnotationActions() {
  return getContext<ReaderAnnotationActions>(READER_ANNOTATION_ACTIONS);
}
