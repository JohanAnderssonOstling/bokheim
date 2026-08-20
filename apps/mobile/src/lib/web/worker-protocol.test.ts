import assert from "node:assert/strict";
import test from "node:test";

import { toWorkerCommandName } from "./worker-protocol.ts";
import { toCloneableWorkerValue } from "./worker-clone.ts";

test("generated command method names map to worker command names", () => {
  assert.equal(toWorkerCommandName("getLibraries"), "get_libraries");
  assert.equal(toWorkerCommandName("backendLinkStatus"), "backend_link_status");
  assert.equal(toWorkerCommandName("updateReadingPosition"), "update_reading_position");
});

test("reactive proxy command arguments become structured-clone-safe plain data", () => {
  const request = new Proxy(
    {
      source_id: "opds-litteraturbanken",
      publication: new Proxy({ edition_id: "lb1728740", work_id: null }, {}),
      formats: ["epub"],
    },
    {},
  );
  const result = toCloneableWorkerValue([request, new Uint8Array([1, 2, 3])]);
  assert.notEqual(result[0], request);
  assert.deepEqual(result[0], {
    source_id: "opds-litteraturbanken",
    publication: { edition_id: "lb1728740", work_id: null },
    formats: ["epub"],
  });
  assert.deepEqual(structuredClone(result), result);
});
