import type { ReaderBook } from "../../lib/shared/types";

export interface ReaderSessionCommands {
  closePdfRangeSession(sessionId: string): Promise<unknown>;
}

export function isPdfReaderBook(book: ReaderBook) {
  const source = book.file;
  if (typeof File !== "undefined" && source instanceof File) {
    return source.type === "application/pdf" || /\.pdf$/i.test(source.name);
  }
  const path = book.fileName ?? (typeof source === "string" ? source : "");
  return /\.pdf(?:$|[?#])/i.test(path);
}

export async function closeReaderBookSource(
  source: ReaderBook["file"],
  commands: ReaderSessionCommands,
) {
  if (typeof source === "string") return;
  if (typeof File !== "undefined" && source instanceof File) return;
  if (typeof source !== "object" || source === null) return;

  const session = source as { kind?: string; sessionId?: string };
  if (!session.sessionId) return;
  if (session.kind === "native_pdf_range")
    await commands.closePdfRangeSession(session.sessionId).catch(() => undefined);
}

export function parseAudiobookTarget(target?: string) {
  const prefix = "audiobook-position-ms:";
  if (!target?.startsWith(prefix)) return undefined;
  const position = Number(target.slice(prefix.length));
  return Number.isSafeInteger(position) && position >= 0 ? position : undefined;
}
