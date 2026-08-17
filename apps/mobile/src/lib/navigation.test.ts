import assert from "node:assert/strict";
import test from "node:test";

import { libraryReaderBookKey, readerRouteCacheKey, universalDestinationForRoute } from "./navigation.ts";
import { ScreenTransitionController } from "./screen-transition.ts";

test("universal web routes retain their destination", () => {
  assert.equal(universalDestinationForRoute("/store"), "store");
  assert.equal(universalDestinationForRoute("/account"), "account");
  assert.equal(universalDestinationForRoute("/settings"), "settings");
  assert.equal(universalDestinationForRoute("/library/example"), null);
  assert.equal(universalDestinationForRoute(null), null);
});

test("reader identities are scoped to their library", () => {
  assert.notEqual(readerRouteCacheKey("first", "same-book"), readerRouteCacheKey("second", "same-book"));
  assert.notEqual(libraryReaderBookKey("first", "same-book"), libraryReaderBookKey("second", "same-book"));
});

test("reader close makes both interactive surfaces exclusive", () => {
  const published: string[] = [];
  const controller = new ScreenTransitionController<string>((state) => published.push(`${state.phase}:${state.reader ?? "none"}`));
  const opening = controller.beginReaderOpen("/library/example", "example\0book", "book");
  assert.equal(controller.commitReader(opening, "example\0book", "book"), true);

  const closing = controller.beginReaderClose("/library/fallback");
  assert.deepEqual(controller.state, { phase: "closing-reader", reader: null });
  assert.equal(closing.destination, "/library/example");
  assert.equal(controller.finishReaderClose(closing.token), true);
  assert.deepEqual(controller.state, { phase: "browser", reader: null });
  assert.deepEqual(published, ["opening-reader:none", "reader:book", "closing-reader:none", "browser:none"]);
});

test("a stale reader open cannot replace the browser or a newer reader", () => {
  const controller = new ScreenTransitionController<string>();
  const stale = controller.beginReaderOpen("/library/one", "one\0old", "old");
  const current = controller.beginReaderOpen("/library/two", "two\0new", "new");

  assert.equal(controller.commitReader(stale, "one\0old", "old"), false);
  assert.equal(controller.commitReader(current, "two\0new", "new"), true);
  assert.deepEqual(controller.state, { phase: "reader", reader: "new" });
  controller.showBrowser();
  assert.equal(controller.commitReader(current, "two\0new", "new"), false);
  assert.deepEqual(controller.state, { phase: "browser", reader: null });
});

test("route restoration consumes the opening handoff without starting competing work", () => {
  const controller = new ScreenTransitionController<string>();
  const opening = controller.beginReaderOpen("/library/example", "example\0book", "book");
  const handoff = controller.handoff("example\0book");

  assert.deepEqual(handoff, { token: opening, reader: "book" });
  assert.equal(controller.beginReaderRestore("example\0book"), null);
  assert.equal(controller.commitReader(handoff!.token, "example\0book", handoff!.reader), true);
  controller.releaseHandoff("example\0book", opening);
  assert.equal(controller.handoff("example\0book"), null);
});
