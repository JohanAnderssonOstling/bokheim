export type FitMode = "width" | "zoom" | "height";

export type PdfPageState = {
  width: number;
  height: number;
  displayWidth: number;
  displayHeight: number;
  rendering: boolean;
  renderedScale: number;
};

export type OutlineItem = {
  key: string;
  label: string;
  destination: unknown;
  depth: number;
  parentKeys: string[];
  hasChildren: boolean;
  disabled?: boolean;
};

export type SearchResult = {
  pageIndex: number;
  excerpt: string;
  startItem: number;
  endItem: number;
  startOffset: number;
  endOffset: number;
  rects: {
    left: number;
    top: number;
    width: number;
    height: number;
  }[];
};

export type PdfSelection = {
  pageIndex: number;
  text: string;
  rects: { left: number; top: number; width: number; height: number }[];
  viewportRect: DOMRect;
};
