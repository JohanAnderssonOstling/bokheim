import { copyFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const mobileRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

for (const file of ["html.js", "pdf.js", "view.js"]) {
  copyFileSync(
    resolve(mobileRoot, "vendor-overrides/foliate-js", file),
    resolve(mobileRoot, "src/vendor/foliate-js", file),
  );
}
