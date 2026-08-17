import type { FoliateBook } from "./types.ts";
import { commands } from "./bindings.ts";

export interface HttpRemoteEpubSource {
  kind: "remote_epub";
  url: string;
}

export interface NativeRemoteEpubSource {
  kind: "native_remote_epub";
  sessionId: string;
  size: number;
}

export type RemoteEpubSource = HttpRemoteEpubSource | NativeRemoteEpubSource;

let rangeSessionRefresh: Promise<void> | undefined;

function refreshStreamingSession(): Promise<void> {
  if (!rangeSessionRefresh) {
    rangeSessionRefresh = (async () => {
      const response = await fetch("/api/auth/web/refresh", { method: "POST", credentials: "same-origin" });
      await response.arrayBuffer();
      if (!response.ok) throw new Error("Your session expired. Sign in again to continue reading.");
    })().finally(() => (rangeSessionRefresh = undefined));
  }
  return rangeSessionRefresh;
}

async function authenticatedRangeFetch(url: string, init: RequestInit): Promise<Response> {
  let response = await fetch(url, { ...init, credentials: "same-origin" });
  if (response.status !== 401) return response;
  await response.arrayBuffer();
  await refreshStreamingSession();
  response = await fetch(url, { ...init, credentials: "same-origin" });
  return response;
}

export class HttpRangeReader {
  size = 0;
  initialized = false;
  private readonly url: string;

  constructor(url: string) {
    this.url = url;
  }

  async init(): Promise<void> {
    if (this.initialized) return;
    const response = await authenticatedRangeFetch(this.url, { method: "HEAD" });
    if (!response.ok) throw new Error(`Could not open remote EPUB (${response.status})`);
    const size = Number(response.headers.get("Content-Length"));
    if (!Number.isSafeInteger(size) || size <= 0) throw new Error("Remote EPUB did not report a valid size");
    this.size = size;
    this.initialized = true;
  }

  async readUint8Array(offset: number, length: number): Promise<Uint8Array> {
    if (!this.initialized) await this.init();
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) || offset < 0 || length < 0 || offset + length > this.size) {
      throw new Error("Remote EPUB range is invalid");
    }
    if (!length) return new Uint8Array();
    const response = await authenticatedRangeFetch(this.url, {
      headers: { Range: `bytes=${offset}-${offset + length - 1}` },
    });
    if (response.status !== 206) throw new Error(`Remote EPUB does not support byte ranges (${response.status})`);
    const bytes = new Uint8Array(await response.arrayBuffer());
    if (bytes.byteLength !== length) throw new Error("Remote EPUB returned an incomplete byte range");
    return bytes;
  }
}

export class NativeRangeReader {
  readonly size: number;
  initialized = true;
  private readonly sessionId: string;

  constructor(source: NativeRemoteEpubSource) {
    this.sessionId = source.sessionId;
    this.size = source.size;
  }

  async init(): Promise<void> {}

  async readUint8Array(offset: number, length: number): Promise<Uint8Array> {
    if (!Number.isSafeInteger(offset) || !Number.isSafeInteger(length) || offset < 0 || length < 0 || offset + length > this.size) {
      throw new Error("Remote EPUB range is invalid");
    }
    if (!length) return new Uint8Array();
    return new Uint8Array(await commands.readRemoteEpubRange(this.sessionId, offset, length));
  }
}

export async function openRemoteEpub(source: RemoteEpubSource): Promise<FoliateBook> {
  const [{ EPUB }, { BlobWriter, configure, TextWriter, ZipReader }] = await Promise.all([
    import("../vendor/foliate-js/epub.js"),
    import("../vendor/foliate-js/vendor/zip.js"),
  ]);
  configure({ useWebWorkers: false });
  const reader = source.kind === "native_remote_epub" ? new NativeRangeReader(source) : new HttpRangeReader(source.url);
  const archive = new ZipReader(reader);
  const entries = await archive.getEntries() as unknown as Array<{ filename: string; uncompressedSize?: number; getData: (writer: unknown) => Promise<unknown> }>;
  const byName = new Map(entries.map((entry) => [entry.filename, entry]));
  const entry = (name: string) => byName.get(name) as { uncompressedSize?: number; getData: (writer: unknown) => Promise<unknown> } | undefined;
  const loader = {
    loadText: (name: string) => entry(name)?.getData(new TextWriter()) ?? null,
    loadBlob: (name: string, type?: string) => entry(name)?.getData(new BlobWriter(type)) ?? null,
    getSize: (name: string) => entry(name)?.uncompressedSize ?? 0,
    sha1: undefined,
  };
  return new EPUB(loader).init() as Promise<FoliateBook>;
}

export function isRemoteEpubSource(value: unknown): value is RemoteEpubSource {
  if (!value || typeof value !== "object") return false;
  const source = value as Partial<RemoteEpubSource>;
  return (source.kind === "remote_epub" && typeof source.url === "string")
    || (source.kind === "native_remote_epub" && typeof source.sessionId === "string" && typeof source.size === "number");
}

export async function closeRemoteEpubSource(source: RemoteEpubSource): Promise<void> {
  if (source.kind === "native_remote_epub") await commands.closeRemoteEpub(source.sessionId);
}
