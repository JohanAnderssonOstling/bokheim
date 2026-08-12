import type { commands as nativeCommands } from "../bindings";
import { takeBrowserImport } from "../epub-picker";
import { callWebBackend } from "./worker-client";
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
        return callWebBackend("import_book", [destination, file, fileName]);
      };
    }
    if (property === "openBook") {
      return async (libraryId: string, contentHash: string) => {
        const blob = await callWebBackend<Blob>("open_book_blob", [libraryId, contentHash]);
        return URL.createObjectURL(blob);
      };
    }
    const command = toWorkerCommandName(property);
    return (...args: unknown[]) => callWebBackend(command, args);
  },
});
