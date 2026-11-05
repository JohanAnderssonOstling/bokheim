import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const source = await readFile(new URL("./dictionary_files.js", import.meta.url), "utf8");
const storage = await import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);

test("OPFS dictionary writes handle short writes and close on failure", async () => {
    let contents = new Uint8Array();
    let closed = 0;
    let flushed = 0;
    let fail = false;
    const handle = {
        truncate() { contents = new Uint8Array(); },
        write(bytes, { at }) {
            if (fail) throw new Error("quota exhausted");
            const count = Math.min(bytes.length, 2);
            const next = new Uint8Array(Math.max(contents.length, at + count));
            next.set(contents);
            next.set(bytes.subarray(0, count), at);
            contents = next;
            return count;
        },
        read(bytes, { at }) {
            const slice = contents.subarray(at, at + bytes.length);
            bytes.set(slice);
            return slice.length;
        },
        getSize() { return contents.length; },
        flush() { flushed += 1; },
        close() { closed += 1; },
    };
    const file = { async createSyncAccessHandle() { return handle; } };
    const dictionary = { async getFileHandle(name) { assert.equal(name, "dictionary.dict.dz"); return file; } };
    const dictionaries = { async getDirectoryHandle(id) { assert.equal(id, "test-id"); return dictionary; } };
    const root = { async getDirectoryHandle(name) { assert.equal(name, "bokheim-dictionaries-v1"); return dictionaries; } };
    const previous = Object.getOwnPropertyDescriptor(globalThis, "navigator");
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: { storage: { async getDirectory() { return root; } } } });
    try {
        await storage.dictionaryWrite("test-id", "dictionary.dict.dz", Uint8Array.of(1, 2, 3, 4, 5));
        assert.deepEqual(contents, Uint8Array.of(1, 2, 3, 4, 5));
        assert.equal(closed, 1);
        assert.equal(flushed, 1);
        const opened = await storage.dictionaryOpen("test-id", "dictionary.dict.dz");
        const selected = new Uint8Array(2);
        assert.equal(storage.dictionaryRead(opened, selected, 2), 2);
        assert.deepEqual(selected, Uint8Array.of(3, 4));
        assert.equal(storage.dictionarySize(opened), 5);
        storage.dictionaryClose(opened);
        fail = true;
        await assert.rejects(storage.dictionaryWrite("test-id", "dictionary.dict.dz", Uint8Array.of(9)), /quota exhausted/);
        assert.equal(closed, 3);
        assert.equal(flushed, 1);
    } finally {
        if (previous) Object.defineProperty(globalThis, "navigator", previous);
        else delete globalThis.navigator;
    }
});

test("removal tolerates absent packs and reports other storage failures", async () => {
    const previous = Object.getOwnPropertyDescriptor(globalThis, "navigator");
    let problem = Object.assign(new Error("gone"), { name: "NotFoundError" });
    Object.defineProperty(globalThis, "navigator", { configurable: true, value: { storage: { async getDirectory() { throw problem; } } } });
    try {
        await storage.dictionaryRemove("missing");
        problem = new Error("storage unavailable");
        await assert.rejects(storage.dictionaryRemove("test-id"), /storage unavailable/);
    } finally {
        if (previous) Object.defineProperty(globalThis, "navigator", previous);
        else delete globalThis.navigator;
    }
});
