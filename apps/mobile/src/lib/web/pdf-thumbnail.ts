const TARGET_WIDTH = 400;
const TARGET_HEIGHT = 640;
const JPEG_QUALITY = 0.86;

function isPdf(file: File, fileName: string): boolean {
  return file.type === "application/pdf" || /\.pdf$/i.test(fileName);
}

function canvasJpeg(canvas: HTMLCanvasElement | OffscreenCanvas): Promise<Blob | null> {
  if (typeof OffscreenCanvas !== "undefined" && canvas instanceof OffscreenCanvas) {
    return canvas.convertToBlob({ type: "image/jpeg", quality: JPEG_QUALITY });
  }
  return new Promise((resolve) =>
    (canvas as HTMLCanvasElement).toBlob(resolve, "image/jpeg", JPEG_QUALITY),
  );
}

/** Renders an opaque, library-sized first-page JPEG without blocking import on failure. */
export async function renderPdfFirstPageJpeg(
  file: File,
  fileName: string,
): Promise<Uint8Array | null> {
  if (!isPdf(file, fileName)) return null;
  let pdf:
    { getPage(pageNumber: number): Promise<any>; destroy(): Promise<void> } | undefined;
  try {
    const pdfjs = await import("../../vendor/foliate-js/vendor/pdfjs/pdf.mjs");
    pdfjs.GlobalWorkerOptions.workerSrc = new URL(
      "/pdfjs/pdf.worker.mjs",
      globalThis.location.href,
    ).toString();
    const openedPdf = await pdfjs.getDocument({
      data: new Uint8Array(await file.arrayBuffer()),
      cMapUrl: new URL("/pdfjs/cmaps/", globalThis.location.href).toString(),
      standardFontDataUrl: new URL(
        "/pdfjs/standard_fonts/",
        globalThis.location.href,
      ).toString(),
      isEvalSupported: false,
    }).promise;
    pdf = openedPdf;
    const page = await openedPdf.getPage(1);
    const natural = page.getViewport({ scale: 1 });
    const scale = Math.min(TARGET_WIDTH / natural.width, TARGET_HEIGHT / natural.height);
    const viewport = page.getViewport({ scale });
    const width = Math.max(1, Math.round(viewport.width));
    const height = Math.max(1, Math.round(viewport.height));
    const canvas =
      typeof document !== "undefined"
        ? document.createElement("canvas")
        : typeof OffscreenCanvas !== "undefined"
          ? new OffscreenCanvas(width, height)
          : null;
    if (!canvas) return null;
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d", { alpha: false }) as
      CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
    if (!context) return null;
    context.fillStyle = "#ffffff";
    context.fillRect(0, 0, canvas.width, canvas.height);
    await page.render({ canvas, canvasContext: context, viewport, background: "#ffffff" })
      .promise;
    const jpeg = await canvasJpeg(canvas);
    if (!jpeg || jpeg.type !== "image/jpeg") return null;
    const bytes = new Uint8Array(await jpeg.arrayBuffer());
    return bytes[0] === 0xff && bytes[1] === 0xd8 ? bytes : null;
  } catch (reason) {
    console.warn("Could not render the PDF cover", reason);
    return null;
  } finally {
    await pdf?.destroy().catch(() => undefined);
  }
}
