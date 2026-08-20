import { commands } from "../bindings.ts";
import type { NativePdfRangeSource, ReaderBook } from "../shared/types.ts";

export function isNativePdfRangeSource(
  file: ReaderBook["file"],
): file is NativePdfRangeSource {
  return (
    typeof file === "object" &&
    file !== null &&
    !(file instanceof File) &&
    "kind" in file &&
    file.kind === "native_pdf_range"
  );
}

export async function closeNativePdfRangeSource(
  source: NativePdfRangeSource,
): Promise<void> {
  await commands.closePdfRangeSession(source.sessionId).catch(() => undefined);
}

// pdf.js defaults to 64KB range chunks; a larger chunk means fewer Tauri IPC
// round trips per page, which matters more on Android's JNI-bridged IPC.
const PDF_RANGE_CHUNK_SIZE = 1 << 18;

/**
 * PDF.js's `PDFDataRangeTransport.requestDataRange` has no error channel: a
 * failed range fetch just never calls `onDataRange`, leaving the reader stuck
 * waiting forever (the "shows 404 then the UI hangs" failure this replaces).
 * `fatal` surfaces any such failure as a promise so callers can bail out and
 * show an error instead of hanging.
 */
export function createNativePdfRangeTransport(pdfjs: any, source: NativePdfRangeSource) {
  let rejectFatal: (reason: unknown) => void = () => undefined;
  const fatal = new Promise<never>((_, reject) => {
    rejectFatal = reject;
  });
  // Nothing observes `fatal` once the reader gives up on the document, so
  // give it a permanent no-op handler to avoid an unhandled rejection.
  fatal.catch(() => undefined);

  class NativePdfRangeTransport extends pdfjs.PDFDataRangeTransport {
    constructor() {
      super(source.size, new Uint8Array(0));
    }
    requestDataRange(begin: number, end: number) {
      commands
        .readPdfRange(source.sessionId, begin, end - begin)
        .then((bytes: number[]) => this.onDataRange(begin, new Uint8Array(bytes)))
        .catch((reason: unknown) =>
          rejectFatal(
            reason instanceof Error
              ? reason
              : new Error(`Could not read the PDF: ${String(reason)}`),
          ),
        );
    }
  }

  return {
    range: new NativePdfRangeTransport(),
    rangeChunkSize: PDF_RANGE_CHUNK_SIZE,
    fatal,
  };
}
