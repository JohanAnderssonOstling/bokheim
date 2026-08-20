import { readerSession } from "$lib/app/reader-session.svelte";

/** Deletes a bookmark from the active reader's library. */
export async function deleteBookmark(bookmarkId: string): Promise<void> {
  await readerSession.deleteBookmark(bookmarkId);
}

export async function addBookmark(cfi: string, label: string) {
  return readerSession.addBookmark(cfi, label);
}
