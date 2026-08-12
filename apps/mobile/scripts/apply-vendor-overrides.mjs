import { copyFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const mobileRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

copyFileSync(
  resolve(mobileRoot, "vendor-overrides/foliate-js/html.js"),
  resolve(mobileRoot, "src/vendor/foliate-js/html.js"),
);
