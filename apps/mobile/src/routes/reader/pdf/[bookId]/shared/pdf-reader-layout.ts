import type { PdfPageState } from "../pdf-reader-types";

export const PDF_COLUMN_GAP = 1;
export const PDF_MAX_PAGE_COLUMNS = 6;
export const PDF_MIN_ZOOM = 0.05;
export const PDF_SURFACE_INSET = 0;
export const PDF_VIEWPORT_SCROLL_OVERLAP_LINES = 2;

export function pageLocator(index: number) {
  return `pdf-page:${Math.max(0, index)}`;
}

export function pageFromLocator(locator?: string | null) {
  if (!locator) return 0;
  const direct = /^pdf-page:(\d+)$/.exec(locator);
  if (direct) return Number(direct[1]);
  const legacy = /^epubcfi\(\/6\/(\d+)/.exec(locator);
  return legacy ? Math.max(0, Math.floor(Number(legacy[1]) / 2) - 1) : 0;
}

export function surfaceSize(
  viewport: HTMLDivElement | undefined,
  surfaceInset: number = PDF_SURFACE_INSET,
) {
  return {
    width: Math.max(1, (viewport?.clientWidth ?? window.innerWidth) - surfaceInset * 2),
    height: Math.max(
      1,
      (viewport?.clientHeight ?? window.innerHeight) - surfaceInset * 2,
    ),
  };
}

export function referencePageSize(pages: PdfPageState[], currentPage: number) {
  const page = pages[currentPage] ?? pages[0];
  return { width: page?.width || 612, height: page?.height || 792 };
}

export function widthFitZoom({
  surfaceWidth,
  columns,
  referenceWidth,
  minZoom,
}: {
  surfaceWidth: number;
  columns: number;
  referenceWidth: number;
  minZoom: number;
}) {
  return Math.max(minZoom, surfaceWidth / columns / Math.max(1, referenceWidth));
}

export function columnsThatFit({
  surfaceWidth,
  columnWidth,
  columnGap = PDF_COLUMN_GAP,
  maxColumns = PDF_MAX_PAGE_COLUMNS,
}: {
  surfaceWidth: number;
  columnWidth: number;
  columnGap?: number;
  maxColumns?: number;
}) {
  const columns = Math.floor(
    (surfaceWidth + columnGap) / (Math.max(1, columnWidth) + columnGap),
  );
  return Math.max(1, Math.min(maxColumns, columns));
}

export function resolveLayout({
  pages,
  currentPage,
  fitMode,
  zoom,
  pageColumns,
  surfaceWidth,
  surfaceHeight,
  columnGap = PDF_COLUMN_GAP,
  maxPageColumns = PDF_MAX_PAGE_COLUMNS,
  minZoom = PDF_MIN_ZOOM,
}: {
  pages: PdfPageState[];
  currentPage: number;
  fitMode: "width" | "zoom" | "height";
  zoom: number;
  pageColumns: number;
  surfaceWidth: number;
  surfaceHeight: number;
  columnGap?: number;
  maxPageColumns?: number;
  minZoom?: number;
}) {
  const page = referencePageSize(pages, currentPage);
  const widestZoom = widthFitZoom({
    surfaceWidth,
    columns: 1,
    referenceWidth: page.width,
    minZoom,
  });
  let columns = pageColumns;
  let resolved = zoom;
  if (fitMode === "width") {
    columns = Math.max(1, Math.min(maxPageColumns, pageColumns));
    resolved = widthFitZoom({
      surfaceWidth,
      columns,
      referenceWidth: page.width,
      minZoom,
    });
  } else {
    resolved = fitMode === "height" ? surfaceHeight / page.height : zoom;
    resolved = Math.max(minZoom, Math.min(widestZoom, resolved));
    columns = columnsThatFit({
      surfaceWidth,
      columnWidth: page.width * resolved,
      columnGap,
      maxColumns: maxPageColumns,
    });
  }
  return { columns, columnWidth: Math.max(1, page.width * resolved), zoom: resolved };
}

export function layoutScale(
  page: PdfPageState,
  columnWidth: number,
  minZoom = PDF_MIN_ZOOM,
) {
  return Math.max(minZoom, columnWidth / Math.max(1, page.width));
}

export function rowStartForScrollTop(
  scrollTop: number,
  pages: PdfPageState[],
  columns: number,
  shellNodes: Array<HTMLElement | undefined>,
) {
  if (!columns || !pages.length) return 0;
  let best = 0;
  for (let index = 0; index < pages.length; index += columns) {
    const node = shellNodes[index];
    if (!node) continue;
    if (node.offsetTop <= scrollTop + 2) best = index;
    else break;
  }
  return best;
}

export function estimateRowLineHeight(
  rowStart: number,
  columns: number,
  pages: PdfPageState[],
  atomicOffsets: Map<number, number[]>,
  fallbackLineHeight = 20,
) {
  if (!columns || !pages.length) return fallbackLineHeight;
  const normalizeRowStart = Math.max(
    0,
    Math.min(
      Math.floor(Math.max(0, pages.length - 1) / columns) * columns,
      Math.max(0, rowStart - (rowStart % columns)),
    ),
  );
  const gatherRowLineHeight = (candidateRowStart: number) => {
    const rowLineHeights: number[] = [];
    for (let index = 0; index < columns; index += 1) {
      const pageIndex = candidateRowStart + index;
      if (pageIndex >= pages.length) break;
      const offsets = atomicOffsets.get(pageIndex);
      if (!offsets || offsets.length < 3) continue;
      for (let cursor = 1; cursor < offsets.length; cursor += 1) {
        const height = offsets[cursor] - offsets[cursor - 1];
        if (height > 0) rowLineHeights.push(height);
      }
    }
    if (!rowLineHeights.length) return 0;
    rowLineHeights.sort((left, right) => left - right);
    return rowLineHeights[Math.floor(rowLineHeights.length / 2)] ?? 0;
  };

  const candidateHeights: number[] = [];
  candidateHeights.push(gatherRowLineHeight(normalizeRowStart));
  for (let step = 1; step <= 2; step += 1) {
    const previous = normalizeRowStart - step * columns;
    if (previous >= 0) candidateHeights.push(gatherRowLineHeight(previous));
    const next = normalizeRowStart + step * columns;
    if (next < pages.length) candidateHeights.push(gatherRowLineHeight(next));
  }

  for (const height of candidateHeights) {
    if (height > 0) return height;
  }

  return fallbackLineHeight;
}

export function lineOffsetsInDocument(
  shellNodes: Array<HTMLElement | undefined>,
  atomicOffsets: Map<number, number[]>,
) {
  const offsets = [0];
  for (const [index, values] of atomicOffsets) {
    const pageTop = shellNodes[index]?.offsetTop;
    if (pageTop === undefined) continue;
    offsets.push(...values.map((offset) => pageTop + offset));
  }
  for (const node of shellNodes) if (node) offsets.push(node.offsetTop);
  offsets.sort((left, right) => left - right);
  return offsets.filter(
    (offset, index) => index === 0 || Math.abs(offset - offsets[index - 1]) > 2,
  );
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}
