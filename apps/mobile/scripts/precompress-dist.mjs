import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { basename, extname, join } from "node:path";
import { promisify } from "node:util";
import zlib from "node:zlib";

/**
 * Writes a `.br` sibling next to every compressible build artifact.
 *
 * Caddy's `file_server { precompressed br }` serves these directly, so the bytes
 * are compressed once here at maximum effort instead of on every request — the
 * assets are content-hashed and immutable, so per-request work on them is work
 * that never needed doing.
 *
 * Brotli alone: every browser that can run this app accepts it, and shipping
 * gzip copies as well would roughly double what the deploy tarball carries to
 * serve a case `encode zstd gzip` already handles on the fly.
 */

const output = fileURLToPath(new URL(`../${process.env.BOKHEIM_FRONTEND_OUT_DIR || "dist"}/`, import.meta.url));

/** Already-compressed containers: recompressing them costs time and grows the file. */
const OPAQUE = new Set([".br", ".gz", ".zst", ".png", ".jpg", ".jpeg", ".webp", ".avif", ".gif", ".ico", ".woff", ".woff2", ".zip", ".epub", ".mp3", ".mp4", ".m4b", ".pfb"]);

/** Below this, framing overhead and the extra request outweigh any saving. */
const MINIMUM_BYTES = 1024;

const brotli = promisify(zlib.brotliCompress);

async function* compressibleFiles(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      yield* compressibleFiles(path);
    } else if (entry.isFile() && !OPAQUE.has(extname(entry.name).toLowerCase())) {
      const { size } = await stat(path);
      if (size >= MINIMUM_BYTES) yield { path, size };
    }
  }
}

/**
 * WASM is dense binary; the text model would mispredict it. Everything else the
 * build emits — JS, CSS, HTML, JSON, maps — is text.
 */
function brotliMode(path) {
  return extname(path).toLowerCase() === ".wasm" ? zlib.constants.BROTLI_MODE_GENERIC : zlib.constants.BROTLI_MODE_TEXT;
}

async function compress(file) {
  const source = await readFile(file.path);
  const compressed = await brotli(source, {
    params: {
      [zlib.constants.BROTLI_PARAM_QUALITY]: zlib.constants.BROTLI_MAX_QUALITY,
      [zlib.constants.BROTLI_PARAM_SIZE_HINT]: source.length,
      [zlib.constants.BROTLI_PARAM_LGWIN]: zlib.constants.BROTLI_MAX_WINDOW_BITS,
      [zlib.constants.BROTLI_PARAM_MODE]: brotliMode(file.path),
    },
  });

  // A sibling that is not smaller would only ever be served to the client's
  // cost, so leave the origin file to answer alone.
  const smaller = compressed.length < source.length;
  if (smaller) await writeFile(`${file.path}.br`, compressed);
  return { ...file, written: smaller, compressedSize: smaller ? compressed.length : source.length };
}

const files = [];
for await (const file of compressibleFiles(output)) files.push(file);

// zlib's asynchronous calls run on the libuv thread pool, so the files overlap
// without any worker plumbing here.
const results = await Promise.all(files.map(compress));

const written = results.filter((result) => result.written);
const origin = results.reduce((total, result) => total + result.size, 0);
const served = results.reduce((total, result) => total + result.compressedSize, 0);

for (const file of [...results].sort((left, right) => right.size - left.size).slice(0, 3)) {
  console.log(`  ${basename(file.path).padEnd(38)} ${(file.size / 1024).toFixed(0).padStart(6)} KB -> ${(file.compressedSize / 1024).toFixed(0).padStart(6)} KB brotli`);
}
console.log(
  `Precompressed ${written.length}/${results.length} files: ${(origin / 1024 / 1024).toFixed(2)} MB served as ` +
    `${(served / 1024 / 1024).toFixed(2)} MB (${(100 - (served * 100) / origin).toFixed(0)}% smaller).`,
);
