import assert from "node:assert/strict";
import test from "node:test";

import { normalizeFoliateTarget } from "./foliate-target.ts";

test("normalizes backend publication-root TOC targets for Foliate", () => {
  assert.equal(
    normalizeFoliateTarget("/OEBPS/Text/chapter.xhtml#section"),
    "OEBPS/Text/chapter.xhtml#section",
  );
});

test("preserves Foliate-native and non-path targets", () => {
  assert.equal(
    normalizeFoliateTarget("OEBPS/Text/chapter.xhtml#section"),
    "OEBPS/Text/chapter.xhtml#section",
  );
  assert.equal(normalizeFoliateTarget("epubcfi(/6/4!/4/2)"), "epubcfi(/6/4!/4/2)");
  assert.equal(normalizeFoliateTarget("//example.com/chapter"), "//example.com/chapter");
});
