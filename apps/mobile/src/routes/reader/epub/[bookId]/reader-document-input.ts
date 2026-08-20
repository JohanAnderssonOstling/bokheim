import { documentRectToViewport } from "./annotation-placement";

/// Movement past this many pixels means the pointer was dragging, not tapping.
const TAP_MOVEMENT_LIMIT_PX = 12;
/// A press held longer than this is a long-press, not a tap.
const TAP_DURATION_LIMIT_MS = 500;
/// Elements that do something of their own, so a tap on them is not a page tap.
const INTERACTIVE_SELECTOR = "a,button,input,textarea,select,img,svg";
/// Keeps the context menu clear of the window edges.
const MENU_EDGE_INSET_PX = 8;
const MENU_WIDTH_PX = 170;
const MENU_HEIGHT_PX = 150;

export interface SectionInputCallbacks {
  /// The user finished a selection gesture; `capture` reads the live selection.
  onSelectionChange: (doc: Document, index: number) => void;
  /// A stationary primary tap on the page body, meaning "toggle the chrome".
  onPageTap: () => void;
  /// A pointer went down anywhere in the page, before any tap is recognised.
  onPointerDown: () => boolean;
  onPointerMove: (event: PointerEvent) => void;
  onContextMenu: (position: { x: number; y: number } | null) => void;
  onImageActivate: (src: string) => void;
  onKeyDown: (event: KeyboardEvent) => void;
  /// Whether a page tap should currently do anything, e.g. false while a panel is open.
  shouldHandleTap: () => boolean;
}

interface TapStart {
  pointerId: number;
  x: number;
  y: number;
  time: number;
  moved: boolean;
}

/**
 * Wires the reader's pointer and key handling into one loaded section document.
 *
 * Sections render in their own iframe documents, so these listeners are attached
 * per section rather than once on the window. The document is discarded with the
 * section, which is what removes them.
 *
 * The tap heuristics exist to tell a page tap apart from the three gestures that
 * share the same pointer sequence: swiping to turn a page, dragging to select
 * text, and long-pressing for the context menu.
 */
export function attachSectionInput(
  doc: Document,
  index: number,
  callbacks: SectionInputCallbacks,
) {
  let tapStart: TapStart | null = null;

  doc.addEventListener("pointerdown", (event) => {
    // A pointer down that dismissed something has consumed the gesture; the
    // press that closes a popup should not also turn into a page tap.
    if (!callbacks.onPointerDown()) {
      tapStart = null;
      return;
    }
    tapStart =
      event.isPrimary && event.button === 0
        ? {
            pointerId: event.pointerId,
            x: event.clientX,
            y: event.clientY,
            time: event.timeStamp,
            moved: false,
          }
        : null;
  });

  doc.addEventListener("pointermove", (event) => {
    if (
      tapStart?.pointerId === event.pointerId &&
      Math.hypot(event.clientX - tapStart.x, event.clientY - tapStart.y) >
        TAP_MOVEMENT_LIMIT_PX
    ) {
      tapStart.moved = true;
    }
    callbacks.onPointerMove(event);
  });

  doc.addEventListener("pointerup", (event) => {
    const start = tapStart;
    tapStart = null;
    // The selection is not final until after this event has been handled.
    setTimeout(() => callbacks.onSelectionChange(doc, index));
    if (!start || start.pointerId !== event.pointerId) return;
    const movement = Math.hypot(event.clientX - start.x, event.clientY - start.y);
    const duration = event.timeStamp - start.time;
    const target = event.target as Element | null;
    if (
      start.moved ||
      movement > TAP_MOVEMENT_LIMIT_PX ||
      duration > TAP_DURATION_LIMIT_MS ||
      target?.closest?.(INTERACTIVE_SELECTOR) ||
      doc.getSelection()?.type === "Range" ||
      !callbacks.shouldHandleTap()
    )
      return;
    callbacks.onPageTap();
  });

  doc.addEventListener("pointercancel", () => (tapStart = null));
  doc.addEventListener("keyup", () => callbacks.onSelectionChange(doc, index));
  doc.addEventListener("keydown", callbacks.onKeyDown);

  doc.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    const selected = doc.getSelection();
    if (!selected?.rangeCount || selected.isCollapsed) {
      callbacks.onContextMenu(null);
      return;
    }
    // Right-clicking away from the selection is not a request to act on it.
    const range = selected.getRangeAt(0);
    const isOverSelection = Array.from(range.getClientRects()).some(
      (rect) =>
        event.clientX >= rect.left &&
        event.clientX <= rect.right &&
        event.clientY >= rect.top &&
        event.clientY <= rect.bottom,
    );
    if (!isOverSelection) {
      callbacks.onContextMenu(null);
      return;
    }
    callbacks.onSelectionChange(doc, index);
    const cursor = documentRectToViewport(doc, {
      left: event.clientX,
      top: event.clientY,
      right: event.clientX,
      bottom: event.clientY,
    });
    callbacks.onContextMenu({
      x: clampToWindow(cursor.left, MENU_WIDTH_PX, window.innerWidth),
      y: clampToWindow(cursor.top, MENU_HEIGHT_PX, window.innerHeight),
    });
  });

  doc.addEventListener("click", (event) => {
    const image = (event.target as Element | null)?.closest?.("img,svg") as
      HTMLImageElement | SVGElement | null;
    if (image instanceof HTMLImageElement && image.currentSrc)
      callbacks.onImageActivate(image.currentSrc);
  });
}

function clampToWindow(value: number, size: number, extent: number) {
  return Math.max(MENU_EDGE_INSET_PX, Math.min(value, extent - size));
}
