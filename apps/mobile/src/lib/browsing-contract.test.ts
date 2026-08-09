import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { FRONTEND_CONTRACT } from "./bindings.ts";

const source = (relative: string) => readFileSync(new URL(relative, import.meta.url), "utf8");

test("book actions and typed book cards stay shared across both browsing frontends", () => {
  assert.deepEqual(FRONTEND_CONTRACT.book_actions, {
    menu_label: "Book actions",
    open_label: "Open",
    download_label: "Download",
    download_in_progress_label: "Download in progress",
    remove_local_copy_label: "Remove local copy",
    hide_label: "Hide from library",
    restore_label: "Add back to library",
  });
  const mobileCard = source("../components/library/BookCard.svelte");
  assert.match(mobileCard, /BOOK_ACTIONS_CONTRACT/);
  assert.match(mobileCard, /<details class="book-actions"[^>]*>/);
  const nativeCard = source("../../../../ui/gpui/browser/src/library/widgets/book_card.rs");
  assert.match(nativeCard, /BookCardRow/);
  assert.match(nativeCard, /components::book_card/);
  assert.match(nativeCard, /ui\.open_reader/);
});

test("Android publication picker mirrors the shared import contract", () => {
  const activity = readFileSync(new URL("../../src-tauri/gen/android/app/src/main/java/se/bokheim/reader/MainActivity.kt", import.meta.url), "utf8");
  for (const extension of FRONTEND_CONTRACT.browsing_content.import_extensions) {
    assert.match(activity, new RegExp(`"${extension}"`), `Android picker is missing .${extension}`);
  }
  assert.match(activity, /"application\/pdf"/);
});
