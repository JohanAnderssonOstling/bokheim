import type { BrowsingPreferencePatch, BrowsingPreferences, LibraryFolderDestination, ReaderPreferencePatch, ReaderPreferences } from "../bindings.ts";

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
  bokheim_apply_browsing_preference_patch(preferences: number, preferencesLength: number, patch: number, patchLength: number): number;
  bokheim_encode_account_request(kind: number, input: number, inputLength: number): number;
  bokheim_decode_account_response(kind: number, input: number, inputLength: number): number;
  bokheim_decode_blob_manifest_response(input: number, inputLength: number, request: number, requestLength: number): number;
  bokheim_encode_sync_exchange(input: number, inputLength: number): number;
  bokheim_decode_sync_exchange(input: number, inputLength: number, validation: number, validationLength: number): number;
  bokheim_project_directories(input: number, inputLength: number): number;
  bokheim_ddc_subject_path(input: number, inputLength: number): number;
  bokheim_lcc_subject_path(input: number, inputLength: number): number;
  bokheim_project_subject_codes(input: number, inputLength: number): number;
  bokheim_unified_subject_paths(input: number, inputLength: number): number;
  bokheim_unified_has_exact_source_code(input: number, inputLength: number): number;
  bokheim_install_taxonomy_overlay(input: number, inputLength: number): number;
  bokheim_select_folder_destinations(input: number, inputLength: number): number;
  bokheim_choose_library_name(input: number, inputLength: number): number;
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

export async function ddcSubjectPath(notation: string): Promise<string | null> {
  const input = new TextEncoder().encode(notation);
  try {
    const value = await transformWithStoredResult(input, (core, pointer, length) => core.bokheim_ddc_subject_path(pointer, length));
    return new TextDecoder().decode(value);
  } catch {
    return null;
  }
}

export async function lccSubjectPath(notation: string): Promise<string | null> {
  const input = new TextEncoder().encode(notation);
  try {
    const value = await transformWithStoredResult(input, (core, pointer, length) => core.bokheim_lcc_subject_path(pointer, length));
    return new TextDecoder().decode(value);
  } catch {
    return null;
  }
}

export async function unifiedSubjectPaths(systemId: "bisac" | "ddc" | "lcc", code: string): Promise<string[]> {
  const input = new TextEncoder().encode(JSON.stringify({ system_id: systemId, code }));
  const value = await transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_unified_subject_paths(pointer, length));
  return JSON.parse(new TextDecoder().decode(value)) as string[];
}

export async function projectSubjectCodes(subjects: unknown[]): Promise<Array<{ system: "bisac" | "ddc" | "lcc"; code: string }>> {
  const input = new TextEncoder().encode(JSON.stringify(subjects));
  const value = await transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_project_subject_codes(pointer, length));
  return JSON.parse(new TextDecoder().decode(value)) as Array<{ system: "bisac" | "ddc" | "lcc"; code: string }>;
}

export async function unifiedHasExactSourceCode(systemId: "bisac" | "ddc" | "lcc", code: string): Promise<boolean> {
  const input = new TextEncoder().encode(JSON.stringify({ system_id: systemId, code }));
  const value = await transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_unified_has_exact_source_code(pointer, length));
  return JSON.parse(new TextDecoder().decode(value)) as boolean;
}

export async function installTaxonomyOverlay(concepts: unknown[]): Promise<void> {
  const input = new TextEncoder().encode(JSON.stringify(concepts));
  await transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_install_taxonomy_overlay(pointer, length));
}

export type FolderDestinationPolicy =
  | { kind: "book"; source_id: string }
  | { kind: "move_folder"; source_parent_id: string; source_path: string };

export async function selectFolderDestinations(
  destinations: LibraryFolderDestination[], policy: FolderDestinationPolicy,
): Promise<LibraryFolderDestination[]> {
  const input = new TextEncoder().encode(JSON.stringify({ destinations, policy }));
  const json = await transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_select_folder_destinations(pointer, length));
  return JSON.parse(new TextDecoder().decode(json)) as LibraryFolderDestination[];
}

export async function chooseLibraryName(
  kind: "folder" | "file", requested: string, occupied: string[],
): Promise<string> {
  const input = new TextEncoder().encode(JSON.stringify({ kind, requested, occupied }));
  const json = await transformWithStoredResult(input, (core, pointer, length) =>
    core.bokheim_choose_library_name(pointer, length));
  return JSON.parse(new TextDecoder().decode(json)) as string;
}

export interface PublicationMetadata {
  title: string | null;
  author: string | null;
  description: string | null;
  subjects: string[];
  subject_paths: string[];
  subject_codes: Array<{ system: "bisac" | "ddc" | "lcc"; code: string }>;
  isbns: string[];
  cover: Uint8Array | null;
  toc: Array<{ title: string; target: string; children: PublicationMetadata["toc"] }>;
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
    if (result.length < 36) throw new Error("The EPUB metadata result is invalid");
    const header = new DataView(result.buffer, result.byteOffset, 36);
    const lengths = Array.from({ length: 9 }, (_, index) => header.getUint32(index * 4, true));
    let offset = 36;
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
    const description = take(lengths[2]);
    const subjects = take(lengths[3]);
    const subjectPaths = take(lengths[4]);
    const subjectCodes = take(lengths[5]);
    const isbns = take(lengths[6]);
    const cover = take(lengths[7]);
    const toc = take(lengths[8]);
    if (offset !== result.length) throw new Error("The EPUB metadata result has trailing data");
    const decoder = new TextDecoder();
    return {
      title: title === null ? null : decoder.decode(title),
      author: author === null ? null : decoder.decode(author),
      description: description === null ? null : decoder.decode(description),
      subjects: subjects === null ? [] : JSON.parse(decoder.decode(subjects)),
      subject_paths: subjectPaths === null ? [] : JSON.parse(decoder.decode(subjectPaths)),
      subject_codes: subjectCodes === null ? [] : JSON.parse(decoder.decode(subjectCodes)),
      isbns: isbns === null ? [] : JSON.parse(decoder.decode(isbns)),
      cover,
      toc: toc === null ? [] : JSON.parse(decoder.decode(toc)),
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

export async function applyBrowsingPreferencePatch(
  preferences: BrowsingPreferences,
  patch: BrowsingPreferencePatch,
): Promise<BrowsingPreferences> {
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
    const status = core.bokheim_apply_browsing_preference_patch(pointer, preferencesJson.length, patchPointer, patchJson.length);
    const result = new Uint8Array(core.memory.buffer, core.bokheim_result_pointer(), core.bokheim_result_length()).slice();
    const value = new TextDecoder().decode(result);
    if (status !== 0) throw new Error(value || "Browsing preference is invalid");
    return JSON.parse(value) as BrowsingPreferences;
  } finally {
    core.bokheim_dealloc(pointer, allocationLength);
  }
}
