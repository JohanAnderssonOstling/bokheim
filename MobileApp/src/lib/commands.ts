import { commands as nativeCommands } from "./bindings";
import { isNativeApp } from "./platform";
import { webDemoCommands } from "./web-demo-commands";

export const commands: typeof nativeCommands = isNativeApp
  ? nativeCommands
  : webDemoCommands as typeof nativeCommands;
