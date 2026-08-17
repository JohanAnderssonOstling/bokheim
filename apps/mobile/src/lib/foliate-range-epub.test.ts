import assert from "node:assert/strict";
import test from "node:test";

import { HttpRangeReader, isRemoteEpubSource } from "./foliate-range-epub.ts";

test("HTTP EPUB reader discovers size and requests exact byte ranges", async () => {
  const publication = new Uint8Array(256).map((_, index) => index);
  const requests: RequestInit[] = [];
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (_input, init = {}) => {
    requests.push(init);
    if (init.method === "HEAD") return new Response(null, { status: 200, headers: { "Content-Length": String(publication.length), "Accept-Ranges": "bytes" } });
    const range = new Headers(init.headers).get("Range");
    assert.equal(range, "bytes=23-41");
    return new Response(publication.slice(23, 42), { status: 206, headers: { "Content-Range": "bytes 23-41/256" } });
  };
  try {
    const reader = new HttpRangeReader("/api/libraries/library/blobs/hash");
    await reader.init();
    assert.equal(reader.size, publication.length);
    assert.deepEqual(await reader.readUint8Array(23, 19), publication.slice(23, 42));
    assert.equal(requests.length, 2);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("remote EPUB source guard rejects ordinary URLs and malformed objects", () => {
  assert.equal(isRemoteEpubSource("/api/book"), false);
  assert.equal(isRemoteEpubSource({ kind: "remote_epub" }), false);
  assert.equal(isRemoteEpubSource({ kind: "remote_epub", url: "/api/book" }), true);
  assert.equal(isRemoteEpubSource({ kind: "native_remote_epub", sessionId: "7", size: 1024 }), true);
  assert.equal(isRemoteEpubSource({ kind: "native_remote_epub", sessionId: "7" }), false);
});

test("HTTP EPUB streaming renews an expired cookie and retries the range", async () => {
  let assetRequests = 0;
  let refreshRequests = 0;
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input, init = {}) => {
    if (String(input) === "/api/auth/web/refresh") {
      refreshRequests += 1;
      return new Response(new Uint8Array(), { status: 200 });
    }
    assetRequests += 1;
    if (assetRequests === 1) return new Response(null, { status: 401 });
    assert.equal(init.credentials, "same-origin");
    return new Response(null, { status: 200, headers: { "Content-Length": "128", "Accept-Ranges": "bytes" } });
  };
  try {
    const reader = new HttpRangeReader("/api/libraries/library/blobs/hash");
    await reader.init();
    assert.equal(reader.size, 128);
    assert.equal(assetRequests, 2);
    assert.equal(refreshRequests, 1);
  } finally {
    globalThis.fetch = originalFetch;
  }
});
