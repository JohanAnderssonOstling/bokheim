import type { ReaderPreferencePatch, ReaderPreferences } from "../bindings.ts";

const wasmUrl = new URL("./generated/bokheim_web_core.wasm", import.meta.url);

interface WebCoreExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  bokheim_alloc(length: number): number;
  bokheim_dealloc(pointer: number, length: number): void;
  bokheim_blake3_hex(input: number, inputLength: number, output: number): void;
  bokheim_normalize_search_text(input: number, inputLength: number, output: number, outputCapacity: number): number;
  bokheim_inspect_epub(input: number, inputLength: number): number;
  bokheim_result_pointer(): number;
  bokheim_result_length(): number;
  bokheim_apply_reader_preference_patch(preferences: number, preferencesLength: number, patch: number, patchLength: number): number;
  bokheim_encode_account_request(kind: number, input: number, inputLength: number): number;
  bokheim_decode_account_response(kind: number, input: number, inputLength: number): number;
  bokheim_decode_blob_manifest_response(input: number, inputLength: number, request: number, requestLength: number): number;
  bokheim_encode_sync_exchange(input: number, inputLength: number): number;
  bokheim_decode_sync_exchange(input: number, inputLength: number, validation: number, validationLength: number): number;
  bokheim_project_directories(input: number, inputLength: number): number;
}

async function transformWithStoredResult(
  input: Uint8Array,
  transform: (core: WebCoreExports, pointer: number, length: number) => number,
): Promise<Uint8Array> {
  const core = await loadCore();
  const pointer = core.bokheim_alloc(input.length);
  try {
    new Uint8Array(core.memory.buffer, pointer, input.length).set(input);
    const status = transform(core, pointer, input.length);
    const result = new Uint8Array(
      core.memory.buffer, core.bokheim_result_pointer(), core.bokheim_result_length(),
    ).slice();
    if (status !== 0) throw new Error(new TextDecoder().decode(result) || "The WASM operation failed");
    return result;
  } finally {
    core.bokheim_dealloc(pointer, input.length);
  }
}

async function transformWithValidation(
  input: Uint8Array,
  validation: unknown,
  transform: (core: WebCoreExports, inputPointer: number, inputLength: number, validationPointer: number, validationLength: number) => number,
): Promise<Uint8Array> {
  const core = await loadCore();
  const validationJson = new TextEncoder().encode(JSON.stringify(validation));
  const pointer = core.bokheim_alloc(input.length + validationJson.length);
  try {
    const validationPointer = pointer + input.length;
    new Uint8Array(core.memory.buffer, pointer, input.length).set(input);
    new Uint8Array(core.memory.buffer, validationPointer, validationJson.length).set(validationJson);
    const status = transform(core, pointer, input.length, validationPointer, validationJson.length);
    const result = new Uint8Array(core.memory.buffer, core.bokheim_result_pointer(), core.bokheim_result_length()).slice();
    if (status !== 0) throw new Error(new TextDecoder().decode(result) || "The WASM operation failed");
    return result;
  } finally {
    core.bokheim_dealloc(pointer, input.length + validationJson.length);
  }
}

export async function encodeAccountRequest(kind: number, value: unknown): Promise<Uint8Array> {
  const input = new TextEncoder().encode(JSON.stringify(value));
  return transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_encode_account_request(kind, pointer, length));
}

export async function decodeAccountResponse<T>(kind: number, bytes: Uint8Array): Promise<T> {
  const json = await transformWithStoredResult(bytes, (core, pointer, length) =>
    core.bokheim_decode_account_response(kind, pointer, length));
  return JSON.parse(new TextDecoder().decode(json)) as T;
}

export async function decodeBlobManifestResponse<T>(bytes: Uint8Array, request: unknown): Promise<T> {
  const json = await transformWithValidation(bytes, request, (core, input, inputLength, validation, validationLength) =>
    core.bokheim_decode_blob_manifest_response(input, inputLength, validation, validationLength));
  return JSON.parse(new TextDecoder().decode(json)) as T;
}

export async function encodeSyncExchange(value: unknown): Promise<Uint8Array> {
  const input = new TextEncoder().encode(JSON.stringify(value));
  return transformWithStoredResult(input, (core, pointer, length) => core.bokheim_encode_sync_exchange(pointer, length));
}

export async function decodeSyncExchange<T>(bytes: Uint8Array, validation: unknown): Promise<T> {
  const json = await transformWithValidation(bytes, validation, (core, input, inputLength, validationPointer, validationLength) =>
    core.bokheim_decode_sync_exchange(input, inputLength, validationPointer, validationLength));
  return JSON.parse(new TextDecoder().decode(json)) as T;
}

export async function projectDirectories<T>(intents: unknown): Promise<T> {
  const input = new TextEncoder().encode(JSON.stringify(intents));
  const json = await transformWithStoredResult(input, (core, pointer, length) => core.bokheim_project_directories(pointer, length));
  return JSON.parse(new TextDecoder().decode(json)) as T;
}

export interface PublicationMetadata {
  title: string | null;
  author: string | null;
  cover: Uint8Array | null;
}

let corePromise: Promise<WebCoreExports> | undefined;

async function loadCore(): Promise<WebCoreExports> {
  if (!corePromise) {
    const moduleBytes = wasmUrl.protocol === "file:" && typeof process !== "undefined"
      ? process.getBuiltinModule("node:fs").promises.readFile(wasmUrl)
      : fetch(wasmUrl).then((response) => {
          if (!response.ok) throw new Error(`Could not load the web core (${response.status})`);
          return response.arrayBuffer();
        });
    corePromise = moduleBytes
      .then((module) => WebAssembly.instantiate(module, {}))
      .then(({ instance }) => instance.exports as WebCoreExports);
  }
  return corePromise;
}

export async function hashPublication(bytes: Uint8Array): Promise<string> {
  const core = await loadCore();
  const allocationLength = bytes.length + 64;
  const inputPointer = core.bokheim_alloc(allocationLength);
  try {
    const outputPointer = inputPointer + bytes.length;
    new Uint8Array(core.memory.buffer, inputPointer, bytes.length).set(bytes);
    core.bokheim_blake3_hex(inputPointer, bytes.length, outputPointer);
    return new TextDecoder("ascii").decode(new Uint8Array(core.memory.buffer, outputPointer, 64));
  } finally {
    core.bokheim_dealloc(inputPointer, allocationLength);
  }
}

export async function normalizeSearchText(value: string): Promise<string> {
  const core = await loadCore();
  const input = new TextEncoder().encode(value);
  const outputCapacity = Math.max(1, input.length * 4);
  const allocationLength = input.length + outputCapacity;
  const pointer = core.bokheim_alloc(allocationLength);
  try {
    const outputPointer = pointer + input.length;
    new Uint8Array(core.memory.buffer, pointer, input.length).set(input);
    const outputLength = core.bokheim_normalize_search_text(
      pointer, input.length, outputPointer, outputCapacity,
    );
    if (outputLength > outputCapacity) throw new Error("Normalized search text exceeded the WASM output buffer");
    return new TextDecoder().decode(new Uint8Array(core.memory.buffer, outputPointer, outputLength));
  } finally {
    core.bokheim_dealloc(pointer, allocationLength);
  }
}

export async function inspectEpubPublication(bytes: Uint8Array): Promise<PublicationMetadata> {
  const core = await loadCore();
  const pointer = core.bokheim_alloc(bytes.length);
  try {
    new Uint8Array(core.memory.buffer, pointer, bytes.length).set(bytes);
    if (core.bokheim_inspect_epub(pointer, bytes.length) !== 0) {
      throw new Error("The selected file is not a readable EPUB");
    }
    const resultPointer = core.bokheim_result_pointer();
    const resultLength = core.bokheim_result_length();
    const result = new Uint8Array(core.memory.buffer, resultPointer, resultLength).slice();
    if (result.length < 12) throw new Error("The EPUB metadata result is invalid");
    const header = new DataView(result.buffer, result.byteOffset, 12);
    const lengths = [header.getUint32(0, true), header.getUint32(4, true), header.getUint32(8, true)];
    let offset = 12;
    const take = (length: number): Uint8Array | null => {
      if (length === 0xffffffff) return null;
      const end = offset + length;
      if (end > result.length) throw new Error("The EPUB metadata result is truncated");
      const value = result.slice(offset, end);
      offset = end;
      return value;
    };
    const title = take(lengths[0]);
    const author = take(lengths[1]);
    const cover = take(lengths[2]);
    if (offset !== result.length) throw new Error("The EPUB metadata result has trailing data");
    const decoder = new TextDecoder();
    return {
      title: title === null ? null : decoder.decode(title),
      author: author === null ? null : decoder.decode(author),
      cover,
    };
  } finally {
    core.bokheim_dealloc(pointer, bytes.length);
  }
}

export async function applyReaderPreferencePatch(
  preferences: ReaderPreferences,
  patch: ReaderPreferencePatch,
): Promise<ReaderPreferences> {
  const core = await loadCore();
  const encoder = new TextEncoder();
  const preferencesJson = encoder.encode(JSON.stringify(preferences));
  const patchJson = encoder.encode(JSON.stringify(patch));
  const allocationLength = preferencesJson.length + patchJson.length;
  const pointer = core.bokheim_alloc(allocationLength);
  try {
    const patchPointer = pointer + preferencesJson.length;
    new Uint8Array(core.memory.buffer, pointer, preferencesJson.length).set(preferencesJson);
    new Uint8Array(core.memory.buffer, patchPointer, patchJson.length).set(patchJson);
    const status = core.bokheim_apply_reader_preference_patch(
      pointer, preferencesJson.length, patchPointer, patchJson.length,
    );
    const result = new Uint8Array(
      core.memory.buffer, core.bokheim_result_pointer(), core.bokheim_result_length(),
    ).slice();
    const value = new TextDecoder().decode(result);
    if (status !== 0) throw new Error(value || "Reader preference is invalid");
    return JSON.parse(value) as ReaderPreferences;
  } finally {
    core.bokheim_dealloc(pointer, allocationLength);
  }
}
