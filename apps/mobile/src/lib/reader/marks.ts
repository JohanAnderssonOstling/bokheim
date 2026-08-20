import type { ReaderAnnotation, ReaderBookmark } from "$lib/shared/types.ts";

type ReaderMarksRuntime = {
  annotations: () => ReaderAnnotation[];
  bookmarks: () => ReaderBookmark[];
  activateAnnotation: (id: string) => void | Promise<void>;
  deleteAnnotation: (id: string) => void | Promise<void>;
  addBookmark: () => void | Promise<void>;
  canAddBookmark: () => boolean;
  openBookmark: (id: string) => void | Promise<void>;
  deleteBookmark: (id: string) => void | Promise<void>;
};

let activeRuntime: ReaderMarksRuntime | undefined;

export function installReaderMarks(runtime: ReaderMarksRuntime) {
  activeRuntime = runtime;
  return () => {
    if (activeRuntime === runtime) activeRuntime = undefined;
  };
}

export const readerAnnotations = () => activeRuntime?.annotations();
export const readerBookmarks = () => activeRuntime?.bookmarks();
export const canAddReaderBookmark = () => activeRuntime?.canAddBookmark() ?? false;
export const activateReaderAnnotation = (id: string) => activeRuntime?.activateAnnotation(id);
export const deleteReaderAnnotation = (id: string) => activeRuntime?.deleteAnnotation(id);
export const addReaderBookmark = () => activeRuntime?.addBookmark();
export const openReaderBookmark = (id: string) => activeRuntime?.openBookmark(id);
export const deleteReaderBookmark = (id: string) => activeRuntime?.deleteBookmark(id);
