interface BokheimAlgorithmExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  bokheim_alloc(length: number): number;
  bokheim_dealloc(pointer: number, length: number): void;
  bokheim_valid_epub_cfi(pointer: number, length: number): number;
  bokheim_valid_reading_position(pointer: number, length: number): number;
}

const encoder = new TextEncoder();
let modulePromise: Promise<BokheimAlgorithmExports> | undefined;
let loadWarningReported = false;

async function instantiateAlgorithms(): Promise<BokheimAlgorithmExports> {
  const response = await fetch("/bokheim_web_algorithms.wasm", { cache: "force-cache" });
  if (!response.ok) throw new Error(`WASM algorithms returned ${response.status}`);

  let instance: WebAssembly.Instance;
  if (response.headers.get("content-type")?.includes("application/wasm")) {
    ({ instance } = await WebAssembly.instantiateStreaming(response, {}));
  } else {
    ({ instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {}));
  }
  return instance.exports as BokheimAlgorithmExports;
}

function algorithms() {
  // SvelteKit retains this ES module across client-side navigation, so both the
  // fetch and WebAssembly compilation happen at most once per page lifetime.
  return modulePromise ??= instantiateAlgorithms();
}

async function callStringAlgorithm(
  value: string,
  select: (exports: BokheimAlgorithmExports) => (pointer: number, length: number) => number,
) {
  const exports = await algorithms();
  const bytes = encoder.encode(value);
  const pointer = exports.bokheim_alloc(bytes.length);
  try {
    new Uint8Array(exports.memory.buffer, pointer, bytes.length).set(bytes);
    return select(exports)(pointer, bytes.length) === 1;
  } finally {
    exports.bokheim_dealloc(pointer, bytes.length);
  }
}

export function validEpubCfi(value: string) {
  return callStringAlgorithm(value, (exports) => exports.bokheim_valid_epub_cfi);
}

export function validReadingPosition(value: string) {
  return callStringAlgorithm(value, (exports) => exports.bokheim_valid_reading_position);
}

/** Returns null when WASM is unavailable so server validation remains the fallback. */
export async function validateReadingPositionLocally(value: string): Promise<boolean | null> {
  try {
    return await validReadingPosition(value);
  } catch (error) {
    if (!loadWarningReported) {
      loadWarningReported = true;
      console.warn("Browser-side Rust validation is unavailable; using server validation.", error);
    }
    return null;
  }
}

