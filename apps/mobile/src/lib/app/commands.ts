import { commands as nativeCommands } from "../bindings";
import { isNativeApp } from "./platform";
import { webCommands } from "../web/web-commands";

/**
 * The UI's single platform boundary. Native builds keep using the generated
 * Tauri bindings while ordinary browsers send the same commands to the web
 * backend worker.
 */
export const commands: typeof nativeCommands = isNativeApp ? nativeCommands : webCommands;
