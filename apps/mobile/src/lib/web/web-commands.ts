import type { commands as nativeCommands } from "../bindings";
import { takeBrowserImport } from "../epub-picker";
import { callWebBackend } from "./worker-client";
import { renderPdfFirstPageJpeg } from "./pdf-thumbnail";
import { toWorkerCommandName } from "./worker-protocol";

type CommandClient = typeof nativeCommands;

/**
 * Keeps the browser implementation aligned with the generated Tauri surface
 * without making the native Rust backend depend on any web-only code.
 */
export const webCommands = new Proxy({} as CommandClient, {
  get(_target, property) {
    if (typeof property !== "string") return undefined;
    if (property === "importBook") {
      return async (destination: unknown, sourcePath: string, fileName: string) => {
        const file = takeBrowserImport(sourcePath);
        if (!file) throw new Error("The selected browser file is no longer available");
        const cover = await renderPdfFirstPageJpeg(file, fileName);
        return callWebBackend("import_book", [destination, file, fileName, cover]);
      };
    }
    if (property === "openBook") {
      return async (libraryId: string, contentHash: string) => {
        const source = await callWebBackend<Blob | string>("open_book_blob", [libraryId, contentHash]);
        return typeof source === "string" ? source : URL.createObjectURL(source);
      };
    }
    const command = toWorkerCommandName(property);
    return (...args: unknown[]) => callWebBackend(command, args);
  },
});
