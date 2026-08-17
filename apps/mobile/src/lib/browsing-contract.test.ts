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
  const nativeCard = source("../../../ui/gpui/browser/src/library/widgets/book_card.rs");
  assert.match(nativeCard, /BookCardRow/);
  assert.match(nativeCard, /components::book_card/);
  assert.match(nativeCard, /ui\.open_reader/);
});

test("file format filters use checked dropdown menus in both frontends", () => {
  const mobileFilter = source("../components/library/FileTypeFilter.svelte");
  assert.match(mobileFilter, /<details[^>]*use:managedMenu/);
  assert.match(mobileFilter, /role="menuitemradio"/);
  assert.match(mobileFilter, /aria-checked=\{option\.value === value\}/);
  assert.doesNotMatch(mobileFilter, /aria-pressed/);

  for (const page of ["folders", "subjects"]) {
    const nativePage = source(`../../../ui/gpui/browser/src/library/pages/${page}/mod.rs`);
    assert.match(nativePage, /dropdown_menu/);
    assert.match(nativePage, /\.checked\(file_type == selected_file_type\)/);
  }
});

test("Android publication picker offers every import format and stages off the UI thread", () => {
  const activity = readFileSync(new URL("../../src-tauri/gen/android/app/src/main/java/se/bokheim/reader/MainActivity.kt", import.meta.url), "utf8");
  assert.match(activity, /type = "\*\/\*"/);
  assert.match(activity, /Intent\.EXTRA_MIME_TYPES/);
  assert.match(activity, /"application\/pdf"/);
  assert.match(activity, /"audio\/x-m4b"/);
  assert.match(activity, /lifecycleScope\.launch/);
  assert.match(activity, /withContext\(Dispatchers\.IO\) \{ stagePublication\(uri\) \}/);
  assert.doesNotMatch(activity, /val \(path, name\) = stagePublication\(uri\)/);
});

test("uncategorized ISBN review renders a windowed queue", () => {
  const subjects = source("../components/library/SubjectsPage.svelte");
  const review = source("../components/library/MetadataReviewPanel.svelte");
  assert.match(subjects, /__uncategorized__/);
  assert.match(subjects, /MetadataReviewPanel/);
  assert.match(review, /filteredReviews\.slice\(startIndex, endIndex\)/);
  assert.match(review, /height: \$\{totalHeight\}px/);
  assert.match(review, /translateY\(\$\{\(startIndex \+ offset\) \* ROW_STRIDE\}px\)/);
});
