export { placeAnnotationPopup } from "../../../../components/shared/readers/annotations/annotation-placement";

/**
 * Converts a rect measured inside a section document into window coordinates.
 *
 * Sections render in an iframe that foliate may scale, so both the frame offset
 * and that scale have to be applied before the rect can position anything at
 * window level.
 */
function toViewport(doc, rect) {
  let frame = doc.defaultView?.frameElement;
  let bounds = frame?.getBoundingClientRect();
  let scaleX =
    bounds && doc.defaultView?.innerWidth ? bounds.width / doc.defaultView.innerWidth : 1;
  let scaleY =
    bounds && doc.defaultView?.innerHeight
      ? bounds.height / doc.defaultView.innerHeight
      : 1;
  return {
    left: (bounds?.left ?? 0) + rect.left * scaleX,
    top: (bounds?.top ?? 0) + rect.top * scaleY,
    right: (bounds?.left ?? 0) + rect.right * scaleX,
    bottom: (bounds?.top ?? 0) + rect.bottom * scaleY,
  };
}

/// The window-space bounding box of an element inside a section document.
export function elementRect(element, doc) {
  return toViewport(doc, element.getBoundingClientRect());
}

/// The window-space bounding box of a range, ignoring its empty client rects.
export function rangeRect(range, doc) {
  let rects = Array.from(range.getClientRects()).filter(
    (rect) => rect.width > 0 && rect.height > 0,
  );
  if (!rects.length) return null;
  return toViewport(doc, {
    left: Math.min(...rects.map((rect) => rect.left)),
    top: Math.min(...rects.map((rect) => rect.top)),
    right: Math.max(...rects.map((rect) => rect.right)),
    bottom: Math.max(...rects.map((rect) => rect.bottom)),
  });
}
