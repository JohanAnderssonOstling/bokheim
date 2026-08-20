/// How close to the top edge a mouse must come to summon hidden chrome.
const TOP_EDGE_REVEAL_PX = 8;

/**
 * Shows and hides the reader's own chrome.
 *
 * Toggling is deferred by a tick because foliate hit-tests annotations on the
 * `click` event that follows `pointerup`: tapping a highlight must open its note
 * instead of hiding the toolbars, and only the later event knows which happened.
 * `cancelPendingToggle` is how the annotation path claims the tap.
 */
export function createReaderChrome() {
  // Android enters immersive reading immediately, so start its own reader chrome
  // hidden to match. Desktop keeps the controls visible and reachable by keyboard.
  let visible = $state(!window.BokheimAndroid);
  let pendingToggle: ReturnType<typeof setTimeout> | undefined;

  function toggleAfterHitTesting() {
    if (pendingToggle) clearTimeout(pendingToggle);
    pendingToggle = setTimeout(() => {
      pendingToggle = undefined;
      visible = !visible;
    });
  }

  function cancelPendingToggle() {
    if (!pendingToggle) return;
    clearTimeout(pendingToggle);
    pendingToggle = undefined;
  }

  /// Brings hidden chrome back when the mouse reaches for it at the top edge.
  function revealAtTopEdge(event: PointerEvent) {
    if (!visible && event.pointerType === "mouse" && event.clientY <= TOP_EDGE_REVEAL_PX)
      visible = true;
  }

  return {
    get visible() {
      return visible;
    },
    get hidden() {
      return !visible;
    },
    toggleAfterHitTesting,
    cancelPendingToggle,
    revealAtTopEdge,
    dispose: cancelPendingToggle,
  };
}
