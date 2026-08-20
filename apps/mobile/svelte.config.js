import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

const sharedIcons = fileURLToPath(new URL("./src/components/shared/icons", import.meta.url));
const phosphorIcons = fileURLToPath(
  new URL("../ui/design-tokens/assets/phosphor-light", import.meta.url),
);

const output = process.env.BOKHEIM_FRONTEND_OUT_DIR || "dist";

export default {
  preprocess: vitePreprocess(),
  kit: {
    alias: {
      "@icons/phosphor": phosphorIcons,
      "@icons/shared": sharedIcons,
    },
    csp: {
      mode: "hash",
      directives: {
        "default-src": ["self"],
        "img-src": ["self", "data:", "blob:"],
        "font-src": ["self", "data:", "blob:"],
        "media-src": ["self", "data:", "blob:"],
        "frame-src": ["self", "data:", "blob:"],
        "style-src": ["self", "unsafe-inline"],
        "script-src": ["self", "wasm-unsafe-eval"],
        "connect-src": ["self", "data:", "blob:", "ipc:", "http://ipc.localhost", "asset:", "http://asset.localhost"],
        "object-src": ["none"],
        "base-uri": ["self"],
      },
    },
    adapter: adapter({
      pages: output,
      assets: output,
      fallback: "index.html",
      strict: true,
    }),
  },
};
