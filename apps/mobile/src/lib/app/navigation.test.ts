import assert from "node:assert/strict";
import test from "node:test";

import { libraryReaderBookKey, universalDestinationForRoute } from "./navigation.ts";

test("universal web routes retain their destination", () => {
  assert.equal(universalDestinationForRoute("/store"), "store");
  assert.equal(universalDestinationForRoute("/account"), "account");
  assert.equal(universalDestinationForRoute("/settings"), "settings");
  assert.equal(universalDestinationForRoute("/library/example"), null);
  assert.equal(universalDestinationForRoute(null), null);
});
