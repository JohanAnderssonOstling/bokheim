import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { chooseLibraryName, ddcSubjectPath, lccSubjectPath, selectFolderDestinations } from "./wasm-core.ts";

interface CoreExports extends WebAssembly.Exports {
  memory: WebAssembly.Memory;
  bokheim_alloc(length: number): number;
  bokheim_dealloc(pointer: number, length: number): void;
  bokheim_blake3_hex(input: number, inputLength: number, output: number): void;
  bokheim_normalize_search_text(input: number, inputLength: number, output: number, outputCapacity: number): number;
}

test("the raw Rust WASM ABI hashes publication bytes", async () => {
  const module = await readFile(new URL("./generated/bokheim_web_core.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(module, {});
  const core = instance.exports as CoreExports;
  const input = new TextEncoder().encode("abc");
  const length = input.length + 64;
  const pointer = core.bokheim_alloc(length);
  try {
    new Uint8Array(core.memory.buffer, pointer, input.length).set(input);
    core.bokheim_blake3_hex(pointer, input.length, pointer + input.length);
    const hash = new TextDecoder().decode(new Uint8Array(core.memory.buffer, pointer + input.length, 64));
    assert.equal(hash, "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85");
  } finally {
    core.bokheim_dealloc(pointer, length);
  }
});

test("the raw Rust WASM ABI uses shared catalog normalization", async () => {
  const module = await readFile(new URL("./generated/bokheim_web_core.wasm", import.meta.url));
  const { instance } = await WebAssembly.instantiate(module, {});
  const core = instance.exports as CoreExports;
  const input = new TextEncoder().encode("  García   Márquez ");
  const length = input.length + 128;
  const pointer = core.bokheim_alloc(length);
  try {
    new Uint8Array(core.memory.buffer, pointer, input.length).set(input);
    const outputLength = core.bokheim_normalize_search_text(pointer, input.length, pointer + input.length, 128);
    const normalized = new TextDecoder().decode(new Uint8Array(core.memory.buffer, pointer + input.length, outputLength));
    assert.equal(normalized, "garcia marquez");
  } finally {
    core.bokheim_dealloc(pointer, length);
  }
});

test("the mobile WASM bridge applies the shared folder destination policy", async () => {
  const destinations = [
    { id: "root", path: "Library" },
    { id: "parent", path: "Shelf" },
    { id: "source", path: "Shelf / Child" },
    { id: "nested", path: "Shelf / Child / Nested" },
    { id: "other", path: "Other" },
  ];
  assert.deepEqual(
    await selectFolderDestinations(destinations, {
      kind: "move_folder", source_parent_id: "parent", source_path: "Shelf / Child",
    }),
    [destinations[0], destinations[4]],
  );
});

test("the mobile WASM bridge uses native collision suffixes", async () => {
  assert.equal(await chooseLibraryName("folder", "Shelf", ["shelf", "Shelf 2"]), "Shelf 3");
  assert.equal(await chooseLibraryName("file", "Book.epub", ["book.epub", "Book 2.epub"]), "Book 3.epub");
});

test("the mobile WASM bridge resolves coarse DDC hierarchy captions", async () => {
  assert.equal(await ddcSubjectPath("515.33"), "DDC / Science / Mathematics / Analysis / 515.33");
  assert.equal(await ddcSubjectPath("not-ddc"), null);
});

test("the mobile WASM bridge resolves coarse LCC outline captions", async () => {
  assert.equal(
    await lccSubjectPath("QA76.73.R3"),
    "LCC / Science / Mathematics / Calculating machines / Electronic computers. Computer science / QA76.73.R3",
  );
  assert.equal(await lccSubjectPath("Incun. 1494 .H46"), null);
});
