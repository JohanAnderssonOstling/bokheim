import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

const host = process.env.TAURI_DEV_HOST;
const unsupportedFoliateFormats = fileURLToPath(
  new URL("./src/lib/unsupported-foliate-formats.js", import.meta.url),
);
const designTokens = JSON.parse(readFileSync(fileURLToPath(new URL("../FrontendContract/design_tokens.json", import.meta.url)), "utf8"));

function tokenVariables(tokens) {
  const variables = (prefix, group, unit = "") => {
    const values = [];
    for (const [name, value] of Object.entries(group)) values.push(`--${prefix}-${name.replaceAll("_", "-")}:${value}${unit};`);
    return values.join("");
  };
  const foundations = variables("spacing", tokens.spacing, "px") + variables("radius", tokens.radius, "px") + variables("size", tokens.size, "px") + variables("type", tokens.typography, "px");
  const readerThemes = Object.entries(tokens.reader_themes).map(([theme, colors]) => variables(`reader-${theme}`, colors)).join("");
  const librarySerif = variables("color", tokens.themes.library_serif);
  const grecoFuturistic = variables("color", tokens.themes.greco_futuristic);
  return `:root{${foundations}${readerThemes}${grecoFuturistic}--font-interface-family:sans-serif;--font-heading-family:serif;}:root[data-application-theme="library_serif"]{${librarySerif}}:root[data-application-theme="greco_futuristic"]{${grecoFuturistic}}`;
}

const sharedDesign = {
  name: "shared-design-contract",
  resolveId(source) {
    if (source === "virtual:design-tokens.css") return "\0virtual:design-tokens.css";
    return null;
  },
  load(id) {
    if (id === "\0virtual:design-tokens.css") return tokenVariables(designTokens);
    return null;
  },
};

// Foliate's generic entry point also references its 13 MB PDF runtime and
// other document parsers. Resolve those optional imports to a small EPUB-only
// guard while preserving upstream's view implementation unchanged.
const foliateEpubOnly = {
  name: "foliate-epub-only",
  enforce: "pre",
  resolveId(source, importer) {
    const isFoliateView = importer?.endsWith("/src/vendor/foliate-js/view.js");
    const optionalFormat = ["./pdf.js", "./comic-book.js", "./fb2.js", "./mobi.js"].includes(source);
    return isFoliateView && optionalFormat ? unsupportedFoliateFormats : null;
  },
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sharedDesign, foliateEpubOnly, svelte()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
