import assert from "node:assert/strict";
import test from "node:test";

import { getWebDatabase } from "./database.ts";
import { hydrateWebTaxonomy } from "./taxonomy.ts";

test("browser taxonomy hydration persists negative resolutions and invalidates obsolete releases", async () => {
  const originalFetch = globalThis.fetch;
  let calls = 0;
  const batchSizes: number[] = [];
  globalThis.fetch = async (_input, init) => {
    calls += 1;
    const request = JSON.parse(String(init?.body)) as { codes: Array<{ system: string; code: string }> };
    batchSizes.push(request.codes.length);
    return new Response(JSON.stringify({
      release_id: calls === 1 ? "release-one" : "release-two",
      results: request.codes.map((code) => ({ ...code, matched_concept_ids: [] })),
      concepts: [],
    }), { headers: { "Content-Type": "application/json" } });
  };
  try {
    await hydrateWebTaxonomy([{ system: "lcc", code: " zzzzweb999 " }]);
    await hydrateWebTaxonomy([{ system: "lcc", code: "ZZZZWEB999" }]);
    assert.equal(calls, 1, "a persisted negative resolution suppresses duplicate requests");

    await hydrateWebTaxonomy([{ system: "ddc", code: "999.99999" }]);
    assert.equal(calls, 2);
    const { db } = await getWebDatabase();
    const rows = db.exec({
      sql: "SELECT system_id,code,server_release_id FROM web_taxonomy_resolutions ORDER BY system_id,code",
      rowMode: "array", returnValue: "resultRows",
    });
    assert.deepEqual(rows, [["ddc", "999.99999", "release-two"]]);

    await hydrateWebTaxonomy(Array.from({ length: 513 }, (_, index) => ({ system: "lcc" as const, code: `ZZZZBATCH${index}` })));
    assert.deepEqual(batchSizes.slice(-2), [512, 1]);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
