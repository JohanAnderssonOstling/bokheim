import assert from "node:assert/strict";
import test from "node:test";

import { toWorkerCommandName } from "./worker-protocol.ts";

test("generated command method names map to worker command names", () => {
  assert.equal(toWorkerCommandName("getLibraries"), "get_libraries");
  assert.equal(toWorkerCommandName("backendLinkStatus"), "backend_link_status");
  assert.equal(toWorkerCommandName("updateReadingPosition"), "update_reading_position");
});
