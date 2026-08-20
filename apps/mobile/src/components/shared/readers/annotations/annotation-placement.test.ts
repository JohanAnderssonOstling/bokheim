import assert from "node:assert/strict";
import test from "node:test";
import { centerAnchor, placeAnnotationPopup } from "./annotation-placement.ts";

const VIEWPORT = { width: 1000, height: 800 };
const POPUP = { width: 200, height: 100 };

function anchorAt(left: number, top: number, right = left, bottom = top) {
  return { left, top, right, bottom };
}

test("prefers below the anchor when it fits", () => {
  const placed = placeAnnotationPopup(anchorAt(400, 300, 600, 320), POPUP, VIEWPORT);
  assert.equal(placed.placement, "below");
  assert.equal(placed.y, 330);
  // Horizontally centred on the anchor.
  assert.equal(placed.x, 400);
});

test("falls back to above when there is no room below", () => {
  const placed = placeAnnotationPopup(anchorAt(400, 700, 600, 760), POPUP, VIEWPORT);
  assert.equal(placed.placement, "above");
  assert.equal(placed.y, 590);
});

test("falls back to the side when neither below nor above fits", () => {
  const tall = { width: 200, height: 700 };
  const placed = placeAnnotationPopup(anchorAt(100, 380, 300, 420), tall, VIEWPORT);
  assert.equal(placed.placement, "right");
});

test("uses the roomiest side when the popup fits nowhere", () => {
  const huge = { width: 2000, height: 2000 };
  const placed = placeAnnotationPopup(anchorAt(100, 600, 200, 700), huge, VIEWPORT);
  // Right of the anchor has 800px against above's 600px, below's 100 and left's 100.
  assert.equal(placed.placement, "right");
});

test("keeps the popup inside the window when the anchor is at an edge", () => {
  const placed = placeAnnotationPopup(anchorAt(0, 0, 10, 10), POPUP, VIEWPORT);
  assert.ok(placed.x >= 12, `expected x within the inset, got ${placed.x}`);
  assert.ok(placed.y >= 12, `expected y within the inset, got ${placed.y}`);

  const corner = placeAnnotationPopup(anchorAt(995, 795, 1000, 800), POPUP, VIEWPORT);
  assert.ok(corner.x + POPUP.width <= VIEWPORT.width - 12);
  assert.ok(corner.y + POPUP.height <= VIEWPORT.height - 12);
});

test("centerAnchor collapses bounds to their midpoint", () => {
  const anchor = centerAnchor({ left: 100, top: 200, right: 500, bottom: 600 });
  assert.deepEqual(anchor, { left: 300, right: 300, top: 400, bottom: 400 });
});
