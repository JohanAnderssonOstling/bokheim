import type { BookCardData } from "../shared/types";

const EVENT = "bokheim:open-reader";
export type ReaderOpenRequest = { book: BookCardData; target?: string };

export function requestReaderOpen(book: BookCardData, target?: string) {
  window.dispatchEvent(
    new CustomEvent<ReaderOpenRequest>(EVENT, { detail: { book, target } }),
  );
}

export function onReaderOpen(handler: (request: ReaderOpenRequest) => void) {
  const listener = (event: Event) =>
    handler((event as CustomEvent<ReaderOpenRequest>).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
}
