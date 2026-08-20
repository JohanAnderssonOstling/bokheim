import { readerSession } from "$lib/app/reader-session.svelte";

/** Deletes an annotation from the active reader's library. */
export async function deleteAnnotation(annotationId: string): Promise<void> {
  await readerSession.deleteAnnotation(annotationId);
}
