import { readFile } from "node:fs/promises";
import { parse } from "../src/vendor/foliate-js/epubcfi.js";

const fixtures = JSON.parse(await readFile(new URL("../../HtmlRenderer/testdata/cfi/foliate-native.json", import.meta.url), "utf8"));

for (const fixture of fixtures) {
  const parsed = parse(fixture.cfi);
  if (!parsed?.parent || !parsed?.start || !parsed?.end) {
    throw new Error(`${fixture.name}: foliate.js did not parse the range CFI`);
  }
  const spineStep = parsed.parent[0]?.[1]?.index;
  if (spineStep !== 2) {
    throw new Error(`${fixture.name}: expected first spine item, got step ${spineStep}`);
  }
}

console.log(`Validated ${fixtures.length} shared foliate.js/native CFI fixtures`);
