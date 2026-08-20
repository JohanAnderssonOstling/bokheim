import type { ViewportRect } from "../../../../components/shared/readers/annotations/annotation-placement";
export {
  centerAnchor,
  placeAnnotationPopup,
} from "../../../../components/shared/readers/annotations/annotation-placement";

/**
 * Converts a rect measured inside a section document into window coordinates.
 *
 * Sections render in an iframe that foliate may scale, so both the frame offset
 * and that scale have to be applied before the rect can position window-level
 * chrome.
 */
export function documentRectToViewport(doc: Document, rect: ViewportRect): ViewportRect {
  const frame = doc.defaultView?.frameElement as HTMLElement | null;
  const frameBounds = frame?.getBoundingClientRect();
  const scaleX =
    frameBounds && doc.defaultView?.innerWidth
      ? frameBounds.width / doc.defaultView.innerWidth
      : 1;
  const scaleY =
    frameBounds && doc.defaultView?.innerHeight
      ? frameBounds.height / doc.defaultView.innerHeight
      : 1;
  return {
    left: (frameBounds?.left ?? 0) + rect.left * scaleX,
    top: (frameBounds?.top ?? 0) + rect.top * scaleY,
    right: (frameBounds?.left ?? 0) + rect.right * scaleX,
    bottom: (frameBounds?.top ?? 0) + rect.bottom * scaleY,
  };
}

/// The window-space bounding box of a range, ignoring its empty client rects.
export function rangeViewportRect(range: Range, doc: Document): ViewportRect | null {
  const rects = Array.from(range.getClientRects()).filter(
    (rect) => rect.width > 0 && rect.height > 0,
  );
  if (!rects.length) return null;
  return documentRectToViewport(doc, {
    left: Math.min(...rects.map((rect) => rect.left)),
    top: Math.min(...rects.map((rect) => rect.top)),
    right: Math.max(...rects.map((rect) => rect.right)),
    bottom: Math.max(...rects.map((rect) => rect.bottom)),
  });
}

function clamp(value: number, minimum: number, maximum: number) {
  return Math.max(minimum, Math.min(value, maximum));
}
