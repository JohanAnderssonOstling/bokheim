export interface ViewportRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export interface PopupSize {
  width: number;
  height: number;
}

export type PopupPlacement = "above" | "below" | "left" | "right";

export interface PopupPosition {
  x: number;
  y: number;
  placement: PopupPlacement;
}

const EDGE_INSET = 12;
const ANCHOR_GAP = 10;

/** Places a popup beside an anchor and keeps it inside the viewport. */
export function placeAnnotationPopup(
  anchor: ViewportRect,
  popup: PopupSize,
  viewport: PopupSize,
): PopupPosition {
  const centerX = (anchor.left + anchor.right) / 2;
  const centerY = (anchor.top + anchor.bottom) / 2;
  const candidates = [
    {
      placement: "below" as const,
      space: viewport.height - anchor.bottom,
      fits: viewport.height - anchor.bottom >= popup.height + ANCHOR_GAP,
      x: centerX - popup.width / 2,
      y: anchor.bottom + ANCHOR_GAP,
    },
    {
      placement: "above" as const,
      space: anchor.top,
      fits: anchor.top >= popup.height + ANCHOR_GAP,
      x: centerX - popup.width / 2,
      y: anchor.top - popup.height - ANCHOR_GAP,
    },
    {
      placement: "right" as const,
      space: viewport.width - anchor.right,
      fits: viewport.width - anchor.right >= popup.width + ANCHOR_GAP,
      x: anchor.right + ANCHOR_GAP,
      y: centerY - popup.height / 2,
    },
    {
      placement: "left" as const,
      space: anchor.left,
      fits: anchor.left >= popup.width + ANCHOR_GAP,
      x: anchor.left - popup.width - ANCHOR_GAP,
      y: centerY - popup.height / 2,
    },
  ];
  const candidate =
    candidates.find((item) => item.fits) ??
    candidates.reduce((best, item) => (item.space > best.space ? item : best));
  return {
    x: clamp(candidate.x, EDGE_INSET, viewport.width - popup.width - EDGE_INSET),
    y: clamp(candidate.y, EDGE_INSET, viewport.height - popup.height - EDGE_INSET),
    placement: candidate.placement,
  };
}

export function centerAnchor(bounds: ViewportRect): ViewportRect {
  const x = (bounds.left + bounds.right) / 2;
  const y = (bounds.top + bounds.bottom) / 2;
  return { left: x, right: x, top: y, bottom: y };
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.max(minimum, Math.min(value, maximum));
}
