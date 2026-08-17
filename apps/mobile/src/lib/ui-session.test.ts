import assert from "node:assert/strict";
import test from "node:test";

import { defaultLibraryPlace, readUiSession, writeUiSession, type BrowseUiSession } from "./ui-session.ts";

class MemoryStorage {
  private values = new Map<string, string>();
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { this.values.set(key, value); }
  removeItem(key: string) { this.values.delete(key); }
}

test("the shared default reopens Folders", () => {
  assert.equal(defaultLibraryPlace().route, "folders");
});

test("each library retains an independent place and universal pages are not persisted", () => {
  const localStorage = new MemoryStorage();
  Object.defineProperty(globalThis, "window", { configurable: true, value: { localStorage } });
  const session: BrowseUiSession = {
    kind: "browse",
    active_library_id: "library-b",
    places: {
      "library-a": { route: "folders", folder_path: [{ id: "fiction", name: "Fiction" }], subject_path: [] },
      "library-b": { route: "authors", folder_path: [], subject_path: [] },
    },
  };
  writeUiSession(session);
  assert.deepEqual(readUiSession(), session);
  assert.equal(JSON.stringify(session).includes("settings"), false);
  Reflect.deleteProperty(globalThis, "window");
});
