import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import { fileURLToPath } from "node:url";
import { cpSync, createReadStream, readFileSync, statSync } from "node:fs";
import { extname, join, normalize } from "node:path";

const host = process.env.TAURI_DEV_HOST;
const webApiProxy = process.env.BOKHEIM_WEB_API_PROXY ?? "https://api.bokheim.se";
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

const crossOriginIsolation = {
  name: "cross-origin-isolation",
  configureServer(server) {
    server.middlewares.use((_request, response, next) => {
      response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
      response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
      next();
    });
  },
  configurePreviewServer(server) {
    server.middlewares.use((_request, response, next) => {
      response.setHeader("Cross-Origin-Opener-Policy", "same-origin");
      response.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
      next();
    });
  },
};

const pdfjsSource = fileURLToPath(new URL("./src/vendor/foliate-js/vendor/pdfjs", import.meta.url));
const pdfMimeTypes = {
  ".css": "text/css; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".bcmap": "application/octet-stream",
  ".pfb": "application/octet-stream",
  ".ttf": "font/ttf",
};

// PDF.js resolves its worker, CMaps, standard fonts, and layer styles by URL.
// Keep that asset tree stable in both Vite development and the static build.
const pdfjsAssets = (() => {
  let outputDirectory = "";
  let isSsrBuild = false;
  return {
    name: "pdfjs-assets",
    configResolved(config) {
      outputDirectory = config.build.outDir;
      isSsrBuild = Boolean(config.build.ssr);
    },
    configureServer(server) {
      server.middlewares.use("/pdfjs", (request, response, next) => {
        const relative = normalize(decodeURIComponent(request.url?.split("?", 1)[0] ?? ""))
          .replace(/^[/\\]+/, "")
          .replace(/^(\.\.[/\\])+/, "");
        const path = join(pdfjsSource, relative);
        try {
          if (!path.startsWith(pdfjsSource) || !statSync(path).isFile()) return next();
          response.setHeader("Content-Type", pdfMimeTypes[extname(path)] ?? "application/octet-stream");
          createReadStream(path).pipe(response);
        } catch {
          next();
        }
      });
    },
    closeBundle() {
      if (!isSsrBuild && outputDirectory) {
        cpSync(pdfjsSource, join(outputDirectory, "pdfjs"), {
          recursive: true,
          filter: (source) => !source.endsWith(".map") && source !== join(pdfjsSource, "pdf.mjs"),
        });
      }
    },
  };
})();

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sharedDesign, crossOriginIsolation, pdfjsAssets, sveltekit()],

  // SQLite ships its own WASM loader and must remain outside Vite's dependency
  // pre-bundle so the same module can initialize inside the backend worker.
  optimizeDeps: {
    exclude: ["@sqlite.org/sqlite-wasm"],
  },

  // The browser backend is launched as a module worker and PDF.js remains a
  // lazy chunk there for catalogue PDF cover rendering.
  worker: {
    format: "es",
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    proxy: {
      "/api": { target: webApiProxy, changeOrigin: true },
    },
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
  preview: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
    proxy: {
      "/api": { target: webApiProxy, changeOrigin: true },
    },
  },
}));
