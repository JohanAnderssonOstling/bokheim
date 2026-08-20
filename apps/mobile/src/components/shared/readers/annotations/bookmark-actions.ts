import { getContext, setContext } from "svelte";
import type { ReaderBookmark } from "$lib/shared/types.ts";

export interface ReaderBookmarkActions {
  readonly items: ReaderBookmark[];
  readonly canAdd: boolean;
  add(): void | Promise<void>;
  open(bookmark: ReaderBookmark): void | Promise<void>;
  delete(bookmark: ReaderBookmark): void | Promise<void>;
}

const READER_BOOKMARK_ACTIONS = Symbol("reader-bookmark-actions");

/** Registers bookmark operations for descendants of one reader session. */
export function provideReaderBookmarkActions(actions: ReaderBookmarkActions) {
  setContext(READER_BOOKMARK_ACTIONS, actions);
}

/** Returns bookmark operations when the current reader provides them. */
export function useReaderBookmarkActions() {
  return getContext<ReaderBookmarkActions>(READER_BOOKMARK_ACTIONS);
}
