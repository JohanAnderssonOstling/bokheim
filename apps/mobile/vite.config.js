import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { fileURLToPath } from "node:url";
import { readFileSync } from "node:fs";

const host = process.env.TAURI_DEV_HOST;
const designTokens = JSON.parse(readFileSync(fileURLToPath(new URL("../ui/design-tokens/design_tokens.json", import.meta.url)), "utf8"));

function tokenVariables(tokens) {
  const variables = (prefix, group, unit = "") => {
    const values = [];
    for (const [name, value] of Object.entries(group)) values.push(`--${prefix}-${name.replaceAll("_", "-")}:${value}${unit};`);
    return values.join("");
  };
  const foundations = variables("spacing", tokens.spacing, "px") + variables("radius", tokens.radius, "px") + variables("size", tokens.size, "px") + variables("type", tokens.typography, "px");
  const palette = (theme, appearance) => variables("color", tokens.themes[theme][appearance]);

  // Two independent axes: which theme, and light or dark. `data-application-appearance`
  // is always written by the app with `System` already resolved, so the CSS
  // never has to consult `prefers-color-scheme` itself.
  const rules = [];
  for (const theme of Object.keys(tokens.themes)) {
    for (const appearance of ["light", "dark"]) {
      rules.push(`:root[data-application-theme="${theme}"][data-application-appearance="${appearance}"]{${palette(theme, appearance)}}`);
    }
  }

  const fallback = palette("greco_futuristic", "light");
  return `:root{${foundations}${fallback}--font-interface-family:sans-serif;--font-heading-family:serif;}` + rules.join("");
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

// Foliate's PDF adapter expects its demo asset tree to be served verbatim and
// cannot be bundled by Vite. PDF remains handled by the app's existing PDF
// reader; only the direct MOBI/FB2/CBZ adapters are enabled here.
const foliateWithoutPdf = {
  name: "foliate-without-pdf",
  enforce: "pre",
  resolveId(source, importer) {
    if (source === "./pdf.js" && importer?.endsWith("/src/vendor/foliate-js/view.js")) {
      return "\0virtual:foliate-pdf-adapter-disabled";
    }
    return null;
  },
  load(id) {
    if (id === "\0virtual:foliate-pdf-adapter-disabled") {
      return `export const makePDF = async () => { throw new Error("PDF files must be opened in the PDF reader") }`;
    }
    return null;
  },
};

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sharedDesign, foliateWithoutPdf, sveltekit()],

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
