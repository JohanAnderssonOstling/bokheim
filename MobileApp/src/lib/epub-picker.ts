import { isTauri } from "@tauri-apps/api/core";
import { appCacheDir, join } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile, remove, writeFile } from "@tauri-apps/plugin-fs";
import { platform } from "@tauri-apps/plugin-os";
import { APP_NAME_LOWER } from "./application-name";

const EPUB_MIME = "application/epub+zip";
const ANDROID_PICKED_EVENT = `${APP_NAME_LOWER}-epub-picked`;

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
  return /\.epub$/i.test(name) ? name : "book.epub";
}

function pickInBrowser(): Promise<PickedEpub | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = `.epub,${EPUB_MIME}`;
    input.addEventListener("change", () => {
      const file = input.files?.[0];
      resolve(file
        ? {
            name: file.name,
            asFile: async () => file,
            asImportPath: async () => null,
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
    const pick = window.BokheimAndroid?.pickEpub;
    if (!pick) {
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
          asFile: async () => new File([await loadBytes()], name, { type: EPUB_MIME }),
          asImportPath: async () => path,
          dispose: async () => remove(path).catch(() => undefined),
        });
      }
    };
    window.addEventListener(ANDROID_PICKED_EVENT, listener);
    try {
      pick(requestId);
    } catch (error) {
      window.removeEventListener(ANDROID_PICKED_EVENT, listener);
      reject(error);
    }
  });
}

/** Opens the platform picker with the narrowest EPUB filter it supports. */
export async function pickEpub(): Promise<PickedEpub | null> {
  if (!isTauri()) return pickInBrowser();

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
      name: "EPUB books",
      // Android's document picker filters by MIME type, not extension.
      extensions: platform() === "android" ? [EPUB_MIME] : ["epub"],
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
      return new File([contents], name, { type: EPUB_MIME });
    },
    asImportPath: async () => {
      if (!path.startsWith("content://")) return path;
      if (stagedPath) return stagedPath;
      const nonce = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      stagedPath = await join(await appCacheDir(), `epub-import-${nonce}.epub`);
      await writeFile(stagedPath, await loadBytes());
      return stagedPath;
    },
    dispose: async () => {
      // Never remove the picker URI: on Android it can reference the original.
      if (stagedPath) await remove(stagedPath).catch(() => undefined);
    },
  };
}
