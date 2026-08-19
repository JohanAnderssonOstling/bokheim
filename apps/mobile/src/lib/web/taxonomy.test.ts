import assert from "node:assert/strict";
import test from "node:test";

import { getWebDatabase } from "./database.ts";
import { hydrateMutationTaxonomy, hydrateWebTaxonomy } from "./taxonomy.ts";

test("browser taxonomy hydration persists negative resolutions and invalidates obsolete releases", async () => {
  const originalFetch = globalThis.fetch;
  const originalNow = Date.now;
  let calls = 0;
  let now = originalNow();
  let advertisedRelease = "release-one";
  let releaseCheckOffline = false;
  const batchSizes: number[] = [];
  Date.now = () => now;
  globalThis.fetch = async (_input, init) => {
    if (init?.method === "HEAD") {
      if (releaseCheckOffline) throw new TypeError("offline");
      return new Response(null, { headers: { ETag: `"${advertisedRelease}"` } });
    }
    calls += 1;
    const request = JSON.parse(String(init?.body)) as { codes: Array<{ system: string; code: string }> };
    batchSizes.push(request.codes.length);
    return new Response(JSON.stringify({
      release_id: advertisedRelease,
      results: request.codes.map((code) => ({ ...code, matched_concept_ids: [] })),
      concepts: [],
    }), { headers: { "Content-Type": "application/json" } });
  };
  try {
    await hydrateWebTaxonomy([{ system: "lcc", code: " zzzzweb999 " }]);
    await hydrateWebTaxonomy([{ system: "lcc", code: "ZZZZWEB999" }]);
    assert.equal(calls, 1, "a persisted negative resolution suppresses duplicate requests");
    const { db } = await getWebDatabase();
    db.exec({
      sql: "INSERT INTO web_taxonomy_concepts(concept_id,concept_json) VALUES (?,?)",
      bind: ["obsolete:test", JSON.stringify({ concept_id: "obsolete:test" })],
    });

    advertisedRelease = "release-two";
    now += 6 * 60 * 60 * 1000 + 1;
    await hydrateWebTaxonomy([{ system: "ddc", code: "999.99999" }]);
    assert.equal(calls, 2);
    const rows = db.exec({
      sql: "SELECT system_id,code,server_release_id FROM web_taxonomy_resolutions ORDER BY system_id,code",
      rowMode: "array", returnValue: "resultRows",
    });
    assert.deepEqual(rows, [["ddc", "999.99999", "release-two"]]);
    assert.deepEqual(db.exec({ sql: "SELECT concept_id FROM web_taxonomy_concepts", rowMode: 0, returnValue: "resultRows" }), []);

    await hydrateWebTaxonomy(Array.from({ length: 513 }, (_, index) => ({ system: "lcc" as const, code: `ZZZZBATCH${index}` })));
    assert.deepEqual(batchSizes.slice(-2), [512, 1]);

    let projectedCodes: Array<{ system: string; code: string }> = [];
    globalThis.fetch = async (_input, init) => {
      if (init?.method === "HEAD") return new Response(null, { headers: { ETag: `"${advertisedRelease}"` } });
      const request = JSON.parse(String(init?.body)) as { codes: Array<{ system: string; code: string }> };
      projectedCodes = request.codes;
      return new Response(JSON.stringify({
        release_id: advertisedRelease,
        results: request.codes.map((code) => ({ ...code, matched_concept_ids: [] })),
        concepts: [],
      }), { headers: { "Content-Type": "application/json" } });
    };
    await hydrateMutationTaxonomy([{
      kind: "metadata",
      value: { publication: { subjects: [
        { name: "History", authority: null, code: null },
        { name: "United States", authority: null, code: null },
        { name: "20th Century", authority: null, code: null },
        { name: "973", authority: "ddc", code: "973" },
        { name: "E184", authority: "lcc", code: "E184" },
      ], subject_paths: [] } },
    }]);
    assert.equal(projectedCodes.some((code) => code.system === "bisac" && code.code.startsWith("HIS036")), true, "inferred regional-history BISAC is remotely hydrated");
    assert.equal(projectedCodes.some((code) => code.system === "ddc" && code.code === "973"), true, "DDC is retained alongside BISAC");
    assert.equal(projectedCodes.some((code) => code.system === "lcc" && code.code === "E184"), true, "LCC is retained alongside BISAC");

    const callsBeforeOfflineHit = calls;
    releaseCheckOffline = true;
    now += 6 * 60 * 60 * 1000 + 1;
    await hydrateWebTaxonomy([{ system: "ddc", code: "999.99999" }]);
    assert.equal(calls, callsBeforeOfflineHit, "a failed release check does not block cached taxonomy hits");
  } finally {
    globalThis.fetch = originalFetch;
    Date.now = originalNow;
  }
});
