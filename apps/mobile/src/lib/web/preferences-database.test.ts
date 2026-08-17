import assert from "node:assert/strict";
import test from "node:test";

import { FRONTEND_CONTRACT } from "../bindings.ts";
import { getWebBrowsingPreferences, getWebReaderPreferences, updateWebBrowsingPreference, updateWebReaderPreference } from "./preferences-database.ts";

test("reader preferences start at contract defaults and persist patches", async () => {
  assert.deepEqual(await getWebReaderPreferences(), FRONTEND_CONTRACT.reader_preferences.defaults);

  const fontSize = await updateWebReaderPreference({ preference: "font_size_px", value: 27 });
  assert.equal(fontSize.font_size_px, 27);
  assert.equal(fontSize.flow, FRONTEND_CONTRACT.reader_preferences.defaults.flow);

  const flow = await updateWebReaderPreference({ preference: "flow", value: "scrolled" });
  assert.equal(flow.font_size_px, 27);
  assert.equal(flow.flow, "scrolled");
  assert.deepEqual(await getWebReaderPreferences(), flow);
});

test("invalid reader preferences are rejected without changing the saved value", async () => {
  const before = await getWebReaderPreferences();
  await assert.rejects(
    updateWebReaderPreference({ preference: "zoom", value: Number.POSITIVE_INFINITY }),
    /invalid type|outside its supported finite range/,
  );
  await assert.rejects(
    updateWebReaderPreference({ preference: "max_column_count", value: 2.5 }),
    /invalid type|integer/,
  );
  assert.deepEqual(await getWebReaderPreferences(), before);
});

test("browsing preferences use shared validation and persist", async () => {
  assert.deepEqual(await getWebBrowsingPreferences(), FRONTEND_CONTRACT.browsing_preferences.defaults);
  assert.equal((await updateWebBrowsingPreference({ preference: "library_view", value: "detailed" })).library_view, "detailed");
  assert.equal((await getWebBrowsingPreferences()).library_view, "detailed");
  await assert.rejects(updateWebBrowsingPreference({ preference: "ui_font_size_px", value: 255 }), /range|invalid/);
  assert.equal((await getWebBrowsingPreferences()).library_view, "detailed");
});
