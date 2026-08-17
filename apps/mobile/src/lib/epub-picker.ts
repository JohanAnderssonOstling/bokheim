import { isTauri } from "@tauri-apps/api/core";
import { appCacheDir, join } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile, remove, writeFile } from "@tauri-apps/plugin-fs";
import { platform } from "@tauri-apps/plugin-os";
import { APP_NAME_LOWER } from "./application-name";
import { BROWSING_CONTENT_CONTRACT } from "./types";

const EPUB_MIME = "application/epub+zip";
const PDF_MIME = "application/pdf";
const BOOK_EXTENSIONS: readonly string[] = BROWSING_CONTENT_CONTRACT.import_extensions;
const ANDROID_PICKED_EVENT = `${APP_NAME_LOWER}-epub-picked`;
const browserImports = new Map<string, File>();

export function stageBrowserImport(file: File): string {
  const token = `browser-upload:${globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`}`;
  browserImports.set(token, file);
  return token;
}

export function takeBrowserImport(token: string): File | undefined {
  const file = browserImports.get(token);
  browserImports.delete(token);
  return file;
}

interface AndroidPickerResult {
  requestId: string;
  path: string | null;
  name: string | null;
  error: string | null;
}

export interface PickedEpub {
  name: string;
  asFile: () => Promise<File>;
  asImportPath: () => Promise<string | null>;
  dispose: () => Promise<void>;
}

function fileNameFromPath(path: string) {
  const withoutQuery = path.split(/[?#]/, 1)[0];
  let decodedPath = withoutQuery;
  try {
    decodedPath = decodeURIComponent(withoutQuery);
  } catch {}
  const name = decodedPath.split(/[\\/:]/).at(-1) || "book.epub";
  const extension = name.split(".").at(-1)?.toLocaleLowerCase();
  return extension && BOOK_EXTENSIONS.includes(extension) ? name : "book.epub";
}

function publicationMime(name: string) {
  const extension = name.split(".").at(-1)?.toLocaleLowerCase();
  if (extension === "epub") return EPUB_MIME;
  if (extension === "pdf") return PDF_MIME;
  if (extension === "fb2") return "application/x-fictionbook+xml";
  if (extension === "cbz") return "application/vnd.comicbook+zip";
  if (extension === "webpub") return "application/webpub+zip";
  if (extension === "html" || extension === "htm") return "text/html";
  if (extension === "xhtml") return "application/xhtml+xml";
  if (extension === "mobi" || extension === "azw" || extension === "azw3") return "application/x-mobipocket-ebook";
  return "application/octet-stream";
}

function pickInBrowser(_importFormats: boolean): Promise<PickedEpub | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = `.epub,.pdf,${EPUB_MIME},${PDF_MIME}`;
    input.addEventListener("change", () => {
      const file = input.files?.[0];
      resolve(file
        ? {
            name: file.name,
            asFile: async () => file,
            asImportPath: async () => stageBrowserImport(file),
            dispose: async () => undefined,
          }
        : null);
    }, { once: true });
    input.addEventListener("cancel", () => resolve(null), { once: true });
    input.click();
  });
}

function pickOnAndroid(): Promise<PickedEpub | null> {
  return new Promise((resolve, reject) => {
    const bridge = window.BokheimAndroid;
    if (!bridge?.pickEpub) {
      reject(new Error("The Android EPUB picker is unavailable."));
      return;
    }

    const requestId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const listener = (event: Event) => {
      const result = (event as CustomEvent<AndroidPickerResult>).detail;
      if (result?.requestId !== requestId) return;
      window.removeEventListener(ANDROID_PICKED_EVENT, listener);
      if (result.error) {
        reject(new Error(result.error));
      } else if (!result.path) {
        resolve(null);
      } else {
        const path = result.path;
        const name = result.name || "book.epub";
        let bytes: Promise<Uint8Array> | null = null;
        const loadBytes = () => (bytes ??= readFile(path));
        resolve({
          name,
          asFile: async () => new File([await loadBytes()], name, { type: publicationMime(name) }),
          asImportPath: async () => path,
          dispose: async () => remove(path).catch(() => undefined),
        });
      }
    };
    window.addEventListener(ANDROID_PICKED_EVENT, listener);
    try {
      // Android WebView bridge methods must be called on the injected object.
      // Extracting the function loses that receiver and WebView rejects it as
      // a method invocation on a non-injected object.
      bridge.pickEpub(requestId);
    } catch (error) {
      window.removeEventListener(ANDROID_PICKED_EVENT, listener);
      reject(error);
    }
  });
}

/** Opens the platform picker with the narrowest EPUB filter it supports. */
export async function pickEpub(importFormats = false): Promise<PickedEpub | null> {
  if (!isTauri()) return pickInBrowser(importFormats);

  if (platform() === "android" && window.BokheimAndroid?.pickEpub) {
    return pickOnAndroid();
  }

  const selected = await open({
    title: "Choose an EPUB",
    multiple: false,
    directory: false,
    pickerMode: "document",
    fileAccessMode: "copy",
    filters: [{
      name: importFormats ? "Books" : "EPUB books",
      // Android's document picker filters by MIME type, not extension.
      extensions: platform() === "android" ? [EPUB_MIME] : (importFormats ? [...BOOK_EXTENSIONS] : ["epub"]),
    }],
  });
  if (!selected) return null;

  const path = selected;
  const name = fileNameFromPath(path);
  let bytes: Promise<Uint8Array> | null = null;
  let stagedPath: string | null = null;
  const loadBytes = () => (bytes ??= readFile(path));
  return {
    name,
    asFile: async () => {
      const contents = await loadBytes();
      return new File([contents], name, { type: publicationMime(name) });
    },
    asImportPath: async () => {
      if (!path.startsWith("content://")) return path;
      if (stagedPath) return stagedPath;
      const nonce = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      const extension = name.split(".").at(-1)?.toLocaleLowerCase();
      const safeExtension = extension && BOOK_EXTENSIONS.includes(extension) ? extension : "epub";
      stagedPath = await join(await appCacheDir(), `book-import-${nonce}.${safeExtension}`);
      await writeFile(stagedPath, await loadBytes());
      return stagedPath;
    },
    dispose: async () => {
      // Never remove the picker URI: on Android it can reference the original.
      if (stagedPath) await remove(stagedPath).catch(() => undefined);
    },
  };
}
