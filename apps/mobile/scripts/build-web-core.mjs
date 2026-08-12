import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const mobileRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = resolve(mobileRoot, "web-core/Cargo.toml");
const result = spawnSync("cargo", ["build", "--manifest-path", manifest, "--target", "wasm32-unknown-unknown", "--release"], {
  cwd: mobileRoot,
  stdio: "inherit",
});
if (result.status !== 0) process.exit(result.status ?? 1);

const generated = resolve(mobileRoot, "src/lib/web/generated");
mkdirSync(generated, { recursive: true });
copyFileSync(
  resolve(mobileRoot, "web-core/target/wasm32-unknown-unknown/release/bokheim_web_core.wasm"),
  resolve(generated, "bokheim_web_core.wasm"),
);
