import assert from "node:assert/strict";
import test from "node:test";

import {
  allSynced,
  needsAttention,
  storageSegments,
  summarizeSync,
} from "./sync-summary.ts";
import type { LibrarySyncStatus } from "./types.ts";

// The fixtures mirror `summary_tests` in `client/app-contract/src/account.rs`,
// which checks the same rule for the desktop frontend.
function status(
  state: LibrarySyncStatus["state"],
  hasUnsyncedChanges: boolean,
  lastSyncedAt: number | null,
): LibrarySyncStatus {
  return {
    library_id: "00000000-0000-0000-0000-000000000000",
    library_name: "Library",
    state,
    server_url: null,
    account_id: null,
    has_unsynced_changes: hasUnsyncedChanges,
    last_synced_at: lastSyncedAt,
  };
}

test("a settled library needs no attention", () => {
  assert.equal(needsAttention(status("synced", false, 10)), false);
  assert.equal(needsAttention(status("synced", true, 10)), true);
  assert.equal(needsAttention(status("synced", false, null)), true);
  assert.equal(needsAttention(status("local_only", false, 10)), true);
  assert.equal(needsAttention(status("remote_missing", false, 10)), true);
});

test("the summary counts libraries and keeps the latest sync", () => {
  const summary = summarizeSync([
    status("synced", false, 30),
    status("synced", false, 70),
    status("local_only", false, 50),
  ]);
  assert.deepEqual(summary, { libraryCount: 3, attentionCount: 1, lastSyncedAt: 70 });
  assert.equal(allSynced(summary), false);
});

test("everything synced requires at least one library", () => {
  assert.equal(allSynced(summarizeSync([])), false);
  assert.equal(allSynced(summarizeSync([status("synced", false, 1)])), true);
});

const copy = {
  unattributed: "Elsewhere on the account",
  reserved: "Reserved by uploads",
  otherLibraries: "Other libraries",
};
const library = (library_id: string, used_bytes: number) => ({ library_id, used_bytes });
const names = new Map([
  ["a", "Alpha"],
  ["b", "Beta"],
]);

test("the quota splits into the libraries the server charges for", () => {
  const segments = storageSegments(
    { used_bytes: 800, reserved_bytes: 100, quota_bytes: 1_000 },
    [library("a", 500), library("b", 200)],
    names,
    copy,
  );
  assert.deepEqual(
    segments.map(({ label, bytes, percent }) => [label, bytes, percent]),
    [
      ["Alpha", 500, 50],
      ["Beta", 200, 20],
      ["Elsewhere on the account", 100, 10],
      ["Reserved by uploads", 100, 10],
    ],
  );
});

test("charged bytes no library accounts for become the remainder", () => {
  const segments = storageSegments(
    { used_bytes: 900, reserved_bytes: 0, quota_bytes: 1_000 },
    [library("a", 400)],
    names,
    copy,
  );
  assert.deepEqual(
    segments.map(({ label, bytes }) => [label, bytes]),
    [
      ["Alpha", 400],
      ["Elsewhere on the account", 500],
    ],
  );
});

test("shares that overrun the charged total never produce a negative remainder", () => {
  const segments = storageSegments(
    { used_bytes: 300, reserved_bytes: 0, quota_bytes: 1_000 },
    [library("a", 400)],
    names,
    copy,
  );
  assert.deepEqual(
    segments.map(({ label, bytes }) => [label, bytes]),
    [["Alpha", 400]],
  );
});

test("a library the device cannot name falls back to its id", () => {
  const segments = storageSegments(
    { used_bytes: 100, reserved_bytes: 0, quota_bytes: 1_000 },
    [library("unknown", 100)],
    names,
    copy,
  );
  assert.deepEqual(
    segments.map(({ label }) => label),
    ["unknown"],
  );
});

test("a device with no quota has nothing to draw", () => {
  assert.deepEqual(
    storageSegments(
      { used_bytes: 10, reserved_bytes: 0, quota_bytes: 0 },
      [library("a", 10)],
      names,
      copy,
    ),
    [],
  );
});

test("libraries past the legend limit are collected together", () => {
  const segments = storageSegments(
    { used_bytes: 600, reserved_bytes: 0, quota_bytes: 1_000 },
    [
      library("a", 100),
      library("b", 100),
      library("c", 100),
      library("d", 100),
      library("e", 100),
      library("f", 100),
    ],
    new Map(),
    copy,
  );
  assert.deepEqual(
    segments.map(({ label, bytes }) => [label, bytes]),
    [
      ["a", 100],
      ["b", 100],
      ["c", 100],
      ["d", 100],
      ["Other libraries", 200],
    ],
  );
});
